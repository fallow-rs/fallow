//! Vital signs computation and snapshot persistence.
//!
//! Vital signs are a fixed set of project-wide metrics computed from available
//! health data. They are always shown as a summary in the health report and can
//! be persisted to `.fallow/snapshots/` for Phase 2b trend tracking.

use std::path::{Path, PathBuf};

/// Number of seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

use crate::health_types::{
    DEFAULT_CYCLOMATIC_CRITICAL, FileHealthScore, HEALTH_SCORE_FORMULA_VERSION,
    HOTSPOT_SCORE_THRESHOLD, HealthScore, HealthScorePenalties, HealthTrend, HotspotEntry,
    RiskProfile, SNAPSHOT_SCHEMA_VERSION, TrendCount, TrendDirection, TrendMetric, TrendPoint,
    VitalSigns, VitalSignsCounts, VitalSignsSnapshot, letter_grade,
};

/// Data sources for computing vital signs.
///
/// Fields are `Option` because not all pipelines run in every health invocation.
pub struct VitalSignsInput<'a> {
    /// All parsed modules (always available).
    pub modules: &'a [fallow_core::extract::ModuleInfo],
    /// Optional file-id allowlist used to restrict per-module aggregates
    /// (cyclomatic distribution, total LOC, unit profiles) to a subset.
    /// Used by `--workspace` and `--group-by` to scope project-wide metrics
    /// to a single workspace package without re-parsing.
    /// `None` includes every module in `modules`.
    pub module_filter: Option<&'a rustc_hash::FxHashSet<fallow_core::discover::FileId>>,
    /// File health scores (available when file_scores/hotspots/targets are computed).
    pub file_scores: Option<&'a [FileHealthScore]>,
    /// Hotspot entries (available when hotspots are computed).
    pub hotspots: Option<&'a [HotspotEntry]>,
    /// Total discovered files (already scoped to the workspace when `--workspace` is set).
    pub total_files: usize,
    /// Analysis results (available when file_scores pipeline ran). When a
    /// `module_filter` is also set, callers should pass workspace-scoped
    /// counts here so `dead_*_pct` denominators line up with the rest of the
    /// metrics.
    pub analysis_counts: Option<AnalysisCounts>,
}

impl<'a> VitalSignsInput<'a> {
    /// Iterate the modules selected by `module_filter`.
    fn selected_modules(&self) -> impl Iterator<Item = &'a fallow_core::extract::ModuleInfo> + '_ {
        let filter = self.module_filter;
        self.modules
            .iter()
            .filter(move |m| filter.is_none_or(|set| set.contains(&m.file_id)))
    }
}

/// Aggregate counts from the analysis pipeline.
#[derive(Clone, Copy)]
pub struct AnalysisCounts {
    pub total_exports: usize,
    pub dead_files: usize,
    pub dead_exports: usize,
    pub unused_deps: usize,
    pub circular_deps: usize,
    pub total_deps: usize,
}

/// Compute vital signs from available health data.
#[expect(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    reason = "vital-sign aggregation keeps the metric definitions in one ordered block; percentile indices, dep counts, hotspot counts, and LOC per file are bounded by project size"
)]
pub fn compute_vital_signs(input: &VitalSignsInput<'_>) -> VitalSigns {
    // Cyclomatic complexity: always available from parsed modules
    let mut all_cyclomatic: Vec<u16> = input
        .selected_modules()
        .flat_map(|m| m.complexity.iter().map(|c| c.cyclomatic))
        .collect();
    all_cyclomatic.sort_unstable();

    let avg_cyclomatic = if all_cyclomatic.is_empty() {
        0.0
    } else {
        let sum: u64 = all_cyclomatic.iter().map(|&c| u64::from(c)).sum();
        (sum as f64 / all_cyclomatic.len() as f64 * 10.0).round() / 10.0
    };
    let critical_complexity_pct = if all_cyclomatic.is_empty() {
        None
    } else {
        let critical_count = all_cyclomatic
            .iter()
            .filter(|&&c| c >= DEFAULT_CYCLOMATIC_CRITICAL)
            .count();
        Some((critical_count as f64 / all_cyclomatic.len() as f64 * 1000.0).round() / 10.0)
    };

    let p90_cyclomatic = if all_cyclomatic.is_empty() {
        0
    } else {
        let idx = (all_cyclomatic.len() as f64 * 0.9).ceil() as usize;
        let idx = idx.min(all_cyclomatic.len()) - 1;
        u32::from(all_cyclomatic[idx])
    };

    // Dead code percentages: only available when analysis pipeline ran
    let (dead_file_pct, dead_export_pct, unused_dep_count, circular_dep_count) =
        if let Some(ref counts) = input.analysis_counts {
            let dfp = if input.total_files > 0 {
                Some((counts.dead_files as f64 / input.total_files as f64 * 1000.0).round() / 10.0)
            } else {
                Some(0.0)
            };
            let dep = if counts.total_exports > 0 {
                Some(
                    (counts.dead_exports as f64 / counts.total_exports as f64 * 1000.0).round()
                        / 10.0,
                )
            } else {
                Some(0.0)
            };
            (
                dfp,
                dep,
                Some(counts.unused_deps as u32),
                Some(counts.circular_deps as u32),
            )
        } else {
            (None, None, None, None)
        };
    let unused_deps_per_k_files = unused_dep_count.map(|count| {
        if input.total_files == 0 {
            0.0
        } else {
            (f64::from(count) / input.total_files as f64 * 10_000.0).round() / 10.0
        }
    });
    let circular_deps_per_k_files = circular_dep_count.map(|count| {
        if input.total_files == 0 {
            0.0
        } else {
            (f64::from(count) / input.total_files as f64 * 10_000.0).round() / 10.0
        }
    });

    // Maintainability average: from file scores
    let maintainability_avg = input.file_scores.and_then(|scores| {
        if scores.is_empty() {
            return None;
        }
        let sum: f64 = scores.iter().map(|s| s.maintainability_index).sum();
        Some((sum / scores.len() as f64 * 10.0).round() / 10.0)
    });
    let maintainability_low_pct = input.file_scores.and_then(|scores| {
        if scores.is_empty() {
            return None;
        }
        let low_count = scores
            .iter()
            .filter(|s| s.maintainability_index < 70.0)
            .count();
        Some((low_count as f64 / scores.len() as f64 * 1000.0).round() / 10.0)
    });

    // Hotspot count: files with score >= threshold
    let hotspot_count = input.hotspots.map(|entries| {
        entries
            .iter()
            .filter(|e| e.score >= HOTSPOT_SCORE_THRESHOLD)
            .count() as u32
    });
    let hotspot_top_pct_count = input.hotspots.map(|entries| {
        if input.total_files == 0 || entries.is_empty() {
            return 0;
        }
        let top_count = (input.total_files as f64 * 0.01).ceil() as usize;
        entries
            .iter()
            .take(top_count.max(1))
            .filter(|entry| entry.score > 0.0)
            .count() as u32
    });

    // Total LOC: always available from parsed modules
    let total_loc: u64 = input
        .selected_modules()
        .map(|m| m.line_offsets.len() as u64)
        .sum();

    // Build raw counts for percentage referents ("63.5% (N of M)")
    let counts = input.analysis_counts.as_ref().map(|ac| VitalSignsCounts {
        total_files: input.total_files,
        total_exports: ac.total_exports,
        dead_files: ac.dead_files,
        dead_exports: ac.dead_exports,
        duplicated_lines: None,
        total_lines: Some(total_loc as usize),
        files_scored: input.file_scores.map(<[_]>::len),
        total_deps: ac.total_deps,
    });

    // Unit size risk profile: bin functions by line count
    let all_line_counts: Vec<u32> = input
        .selected_modules()
        .flat_map(|m| m.complexity.iter().map(|c| c.line_count))
        .collect();
    let functions_over_60_loc_per_k = if all_line_counts.is_empty() {
        None
    } else {
        let over_60 = all_line_counts
            .iter()
            .filter(|&&line_count| line_count > 60)
            .count();
        Some((over_60 as f64 / all_line_counts.len() as f64 * 10_000.0).round() / 10.0)
    };
    let unit_size_profile = if all_line_counts.is_empty() {
        None
    } else {
        Some(compute_size_risk_profile(&all_line_counts))
    };

    // Unit interfacing risk profile: bin functions by param count
    let unit_interfacing_profile = if all_cyclomatic.is_empty() {
        None
    } else {
        let all_param_counts: Vec<u8> = input
            .selected_modules()
            .flat_map(|m| m.complexity.iter().map(|c| c.param_count))
            .collect();
        Some(compute_interfacing_risk_profile(&all_param_counts))
    };

    // Coupling concentration: p95 fan-in and % of files above it
    let (p95_fan_in, coupling_high_pct) = if let Some(scores) = input.file_scores {
        compute_coupling_concentration(scores)
    } else {
        (None, None)
    };

    VitalSigns {
        dead_file_pct,
        dead_export_pct,
        avg_cyclomatic,
        critical_complexity_pct,
        p90_cyclomatic,
        duplication_pct: None, // Lazy: only set if duplication pipeline was run
        hotspot_count,
        hotspot_top_pct_count,
        maintainability_avg,
        maintainability_low_pct,
        unused_dep_count,
        unused_deps_per_k_files,
        circular_dep_count,
        circular_deps_per_k_files,
        counts,
        unit_size_profile,
        functions_over_60_loc_per_k,
        unit_interfacing_profile,
        p95_fan_in,
        coupling_high_pct,
        total_loc,
    }
}

/// Compute unit size risk profile from function line counts.
///
/// Bins: low risk (1-15 LOC), medium risk (16-30), high risk (31-60), very high risk (>60).
fn compute_size_risk_profile(line_counts: &[u32]) -> RiskProfile {
    if line_counts.is_empty() {
        return RiskProfile {
            low_risk: 0.0,
            medium_risk: 0.0,
            high_risk: 0.0,
            very_high_risk: 0.0,
        };
    }
    let total = line_counts.len() as f64;
    let low = line_counts.iter().filter(|&&lc| lc <= 15).count() as f64;
    let medium = line_counts
        .iter()
        .filter(|&&lc| (16..=30).contains(&lc))
        .count() as f64;
    let high = line_counts
        .iter()
        .filter(|&&lc| (31..=60).contains(&lc))
        .count() as f64;
    let very_high = line_counts.iter().filter(|&&lc| lc > 60).count() as f64;
    RiskProfile {
        low_risk: (low / total * 1000.0).round() / 10.0,
        medium_risk: (medium / total * 1000.0).round() / 10.0,
        high_risk: (high / total * 1000.0).round() / 10.0,
        very_high_risk: (very_high / total * 1000.0).round() / 10.0,
    }
}

/// Compute unit interfacing risk profile from function parameter counts.
///
/// Bins: low risk (0-2 params), medium risk (3-4), high risk (5-6), very high risk (>=7).
fn compute_interfacing_risk_profile(param_counts: &[u8]) -> RiskProfile {
    if param_counts.is_empty() {
        return RiskProfile {
            low_risk: 0.0,
            medium_risk: 0.0,
            high_risk: 0.0,
            very_high_risk: 0.0,
        };
    }
    let total = param_counts.len() as f64;
    let low = param_counts.iter().filter(|&&pc| pc <= 2).count() as f64;
    let medium = param_counts
        .iter()
        .filter(|&&pc| (3..=4).contains(&pc))
        .count() as f64;
    let high = param_counts
        .iter()
        .filter(|&&pc| (5..=6).contains(&pc))
        .count() as f64;
    let very_high = param_counts.iter().filter(|&&pc| pc >= 7).count() as f64;
    RiskProfile {
        low_risk: (low / total * 1000.0).round() / 10.0,
        medium_risk: (medium / total * 1000.0).round() / 10.0,
        high_risk: (high / total * 1000.0).round() / 10.0,
        very_high_risk: (very_high / total * 1000.0).round() / 10.0,
    }
}

/// Compute coupling concentration from file health scores.
///
/// Returns (p95_fan_in, coupling_high_pct) where coupling_high_pct is the
/// percentage of files with fan-in above the effective threshold (max(p95_fan_in, 10)).
#[expect(
    clippy::cast_possible_truncation,
    reason = "fan-in values are bounded by project size"
)]
fn compute_coupling_concentration(scores: &[FileHealthScore]) -> (Option<u32>, Option<f64>) {
    if scores.is_empty() {
        return (None, None);
    }
    let mut fan_ins: Vec<usize> = scores.iter().map(|s| s.fan_in).collect();
    fan_ins.sort_unstable();
    let idx = (fan_ins.len() as f64 * 0.95).ceil() as usize;
    let idx = idx.min(fan_ins.len()) - 1;
    let p95 = fan_ins[idx] as u32;

    // Use a floor of 10 for the "high coupling" threshold to avoid flagging
    // small projects where p95 fan-in is naturally low
    let threshold = (p95 as usize).max(10);
    let high_count = fan_ins.iter().filter(|&&fi| fi > threshold).count();
    let high_pct = (high_count as f64 / fan_ins.len() as f64 * 1000.0).round() / 10.0;

    (Some(p95), Some(high_pct))
}

/// Compute a project-level health score from vital signs.
///
/// The score starts at 100 and subtracts penalties for each metric.
/// Missing metrics (from pipelines that didn't run) don't penalize.
/// `total_files` is used to normalize the hotspot count penalty.
pub fn compute_health_score(vs: &VitalSigns, total_files: usize) -> HealthScore {
    // Round each penalty to 1dp BEFORE subtracting so that JSON consumers
    // can reproduce the score as `100 - sum(penalties)`.
    let round1 = |v: f64| -> f64 { (v * 10.0).round() / 10.0 };

    let mut score = 100.0_f64;

    // Dead file penalty: 0.2 points per percent, max 15
    let dead_files_penalty = vs.dead_file_pct.map(|dfp| round1((dfp * 0.2).min(15.0)));
    if let Some(p) = dead_files_penalty {
        score -= p;
    }

    // Dead export penalty: 0.2 points per percent, max 15
    let dead_exports_penalty = vs.dead_export_pct.map(|dep| round1((dep * 0.2).min(15.0)));
    if let Some(p) = dead_exports_penalty {
        score -= p;
    }

    // Complexity penalty: prefer scale-invariant critical-complexity density
    // when current vital signs provide it. Fall back to the legacy average for
    // older snapshots/tests that predate the tail metric.
    let complexity_penalty = if let Some(critical_pct) = vs.critical_complexity_pct {
        round1((critical_pct * 4.0).min(20.0))
    } else {
        round1(((vs.avg_cyclomatic - 1.5).max(0.0) * 5.0).min(20.0))
    };
    score -= complexity_penalty;

    // P90 is retained for backward-compatible output, but new runs fold
    // complexity tail risk into the scale-invariant complexity penalty.
    let p90_penalty = if vs.critical_complexity_pct.is_some() {
        0.0
    } else {
        round1((f64::from(vs.p90_cyclomatic) - 10.0).clamp(0.0, 10.0))
    };
    score -= p90_penalty;

    // Maintainability penalty: prefer percentage of low-MI files over mean MI
    // so a large low-quality tail is not diluted by many trivial files.
    let maintainability_penalty = if let Some(low_pct) = vs.maintainability_low_pct {
        Some(round1((low_pct * 1.5).min(15.0)))
    } else {
        vs.maintainability_avg
            .map(|mi| round1(((70.0 - mi).max(0.0) * 0.5).min(15.0)))
    };
    if let Some(p) = maintainability_penalty {
        score -= p;
    }

    // Hotspot penalty: prefer coverage of the top 1% within-project ranking.
    // The legacy fixed score threshold can be unreachable when churn and
    // density maxima live in different files; scoring against the percentile
    // bucket lets the dimension use its full 10-point budget.
    let hotspot_penalty = if let Some(top_pct_count) = vs.hotspot_top_pct_count {
        if total_files > 0 {
            let top_pct_bucket = (total_files as f64 * 0.01).ceil().max(1.0);
            Some(round1(
                (f64::from(top_pct_count) / top_pct_bucket * 10.0).min(10.0),
            ))
        } else {
            Some(0.0)
        }
    } else {
        vs.hotspot_count.map(|hc| {
            if total_files > 0 {
                round1((f64::from(hc) / total_files as f64 * 200.0).min(10.0))
            } else {
                0.0
            }
        })
    };
    if let Some(p) = hotspot_penalty {
        score -= p;
    }

    // Unused dep penalty: prefer density per 1k files, cap 25.
    let unused_deps_penalty = if let Some(per_k) = vs.unused_deps_per_k_files {
        Some(round1((per_k * 0.5).min(25.0)))
    } else {
        vs.unused_dep_count
            .map(|ud| round1(f64::from(ud).min(10.0)))
    };
    if let Some(p) = unused_deps_penalty {
        score -= p;
    }

    // Circular dep penalty: prefer density per 1k files, cap 25.
    let circular_deps_penalty = if let Some(per_k) = vs.circular_deps_per_k_files {
        Some(round1((per_k * 0.5).min(25.0)))
    } else {
        vs.circular_dep_count
            .map(|cd| round1(f64::from(cd).min(10.0)))
    };
    if let Some(p) = circular_deps_penalty {
        score -= p;
    }

    // Unit size penalty: prefer functions >60 LOC per 1k functions. The legacy
    // percentage floor diluted thousands of oversized functions in large repos.
    let unit_size_penalty = if let Some(per_k) = vs.functions_over_60_loc_per_k {
        Some(round1((per_k * 0.5).min(10.0)))
    } else {
        vs.unit_size_profile
            .as_ref()
            .map(|profile| round1(((profile.very_high_risk - 5.0).max(0.0) * 0.5).min(10.0)))
    };
    if let Some(p) = unit_size_penalty {
        score -= p;
    }

    // Coupling concentration penalty: prefer the percentage of high fan-in files
    // over p95 so heavy-tailed hubs above p99 still contribute.
    let coupling_penalty = if let Some(high_pct) = vs.coupling_high_pct {
        Some(round1((high_pct * 0.5).min(5.0)))
    } else {
        vs.p95_fan_in
            .map(|p95| round1(((f64::from(p95) - 30.0).max(0.0) * 0.25).min(5.0)))
    };
    if let Some(p) = coupling_penalty {
        score -= p;
    }

    // Duplication penalty: 1 point per percent above 5%, max 10
    let duplication_penalty = vs
        .duplication_pct
        .map(|dp| round1(((dp - 5.0).max(0.0) * 1.0).min(10.0)));
    if let Some(p) = duplication_penalty {
        score -= p;
    }

    let score = (score * 10.0).round() / 10.0;
    let score = score.clamp(0.0, 100.0);
    let grade = letter_grade(score);

    HealthScore {
        formula_version: HEALTH_SCORE_FORMULA_VERSION,
        score,
        grade,
        penalties: HealthScorePenalties {
            dead_files: dead_files_penalty,
            dead_exports: dead_exports_penalty,
            complexity: complexity_penalty,
            p90_complexity: p90_penalty,
            maintainability: maintainability_penalty,
            hotspots: hotspot_penalty,
            unused_deps: unused_deps_penalty,
            circular_deps: circular_deps_penalty,
            unit_size: unit_size_penalty,
            coupling: coupling_penalty,
            duplication: duplication_penalty,
        },
    }
}

/// Build the raw counts for a snapshot.
pub fn build_counts(input: &VitalSignsInput<'_>) -> VitalSignsCounts {
    let (total_exports, dead_files, dead_exports, total_deps) = input
        .analysis_counts
        .as_ref()
        .map_or((0, 0, 0, 0), |counts| {
            (
                counts.total_exports,
                counts.dead_files,
                counts.dead_exports,
                counts.total_deps,
            )
        });

    let total_lines: usize = input.selected_modules().map(|m| m.line_offsets.len()).sum();

    VitalSignsCounts {
        total_files: input.total_files,
        total_exports,
        dead_files,
        dead_exports,
        duplicated_lines: None,
        total_lines: Some(total_lines),
        files_scored: input.file_scores.map(<[_]>::len),
        total_deps,
    }
}

/// Get the current git SHA (short form).
fn git_sha(root: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root);
    fallow_core::git_env::clear_ambient_git_env(&mut command);
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Get the current git branch name.
fn git_branch(root: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root);
    fallow_core::git_env::clear_ambient_git_env(&mut command);
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            // Detached HEAD returns "HEAD" — treat as None
            if name == "HEAD" { None } else { Some(name) }
        })
}

/// Build a snapshot from vital signs and input data.
pub fn build_snapshot(
    vital_signs: VitalSigns,
    counts: VitalSignsCounts,
    root: &Path,
    shallow_clone: bool,
    health_score: Option<&HealthScore>,
    coverage_model: Option<crate::health_types::CoverageModel>,
) -> VitalSignsSnapshot {
    let now = chrono_timestamp();

    VitalSignsSnapshot {
        snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: now,
        git_sha: git_sha(root),
        git_branch: git_branch(root),
        shallow_clone,
        vital_signs,
        counts,
        score: health_score.map(|s| s.score),
        grade: health_score.map(|s| s.grade.to_string()),
        coverage_model,
    }
}

/// ISO 8601 UTC timestamp without external chrono dependency.
#[expect(
    clippy::redundant_pub_crate,
    reason = "module is private; pub(crate) documents the cross-module (impact, audit) reuse intent"
)]
pub(crate) fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    // Simple UTC conversion (no leap seconds, good enough for timestamps)
    let days = secs / SECS_PER_DAY;
    let time_secs = secs % SECS_PER_DAY;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Convert days since epoch to y/m/d
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
const fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's date library (public domain)
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Save a snapshot to disk.
///
/// If `path` is `None`, writes to `.fallow/snapshots/{timestamp}.json`.
/// Creates parent directories as needed.
pub fn save_snapshot(
    snapshot: &VitalSignsSnapshot,
    root: &Path,
    explicit_path: Option<&Path>,
) -> Result<PathBuf, String> {
    let path = explicit_path.map_or_else(
        || {
            let dir = root.join(".fallow").join("snapshots");
            // Use the snapshot timestamp for the filename (replace colons for Windows compat)
            let filename = snapshot.timestamp.replace(':', "-");
            dir.join(format!("{filename}.json"))
        },
        Path::to_path_buf,
    );

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create snapshot directory: {e}"))?;
    }

    let json =
        serde_json::to_string_pretty(snapshot).map_err(|e| format!("failed to serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write snapshot: {e}"))?;

    Ok(path)
}

/// Load all snapshots from the default snapshot directory, sorted by timestamp ascending.
///
/// Corrupt or unreadable files are skipped with a warning to stderr.
/// Returns an empty vec if the directory does not exist.
pub fn load_snapshots(root: &Path) -> Vec<VitalSignsSnapshot> {
    let dir = root.join(".fallow").join("snapshots");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut snapshots = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<VitalSignsSnapshot>(&content) {
                    Ok(snap) => snapshots.push(snap),
                    Err(e) => {
                        eprintln!("warning: skipping corrupt snapshot {}: {e}", path.display());
                    }
                },
                Err(e) => {
                    eprintln!("warning: could not read snapshot {}: {e}", path.display());
                }
            }
        }
    }

    // Sort by timestamp (ISO 8601 sorts lexicographically)
    snapshots.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    snapshots
}

/// Tolerance for treating a metric delta as "stable" rather than improving/declining.
const TREND_TOLERANCE: f64 = 0.5;

/// Compute a trend comparison between the current run and the most recent snapshot.
///
/// Uses the stored `score` field from the snapshot (never re-derives it).
/// Returns `None` if no snapshots are available.
#[expect(
    clippy::too_many_lines,
    reason = "trend computation compares many metric dimensions"
)]
pub fn compute_trend(
    current_vs: &VitalSigns,
    current_counts: &VitalSignsCounts,
    current_score: Option<f64>,
    snapshots: &[VitalSignsSnapshot],
) -> Option<HealthTrend> {
    let prev = snapshots.last()?;

    let compared_to = TrendPoint {
        timestamp: prev.timestamp.clone(),
        git_sha: prev.git_sha.clone(),
        score: prev.score,
        grade: prev.grade.clone(),
        coverage_model: prev.coverage_model.clone(),
        snapshot_schema_version: Some(prev.snapshot_schema_version),
    };

    let mut metrics = Vec::new();

    // Health Score — higher is better
    if let (Some(prev_score), Some(cur_score)) = (prev.score, current_score) {
        metrics.push(make_metric(
            "score",
            "Health Score",
            prev_score,
            cur_score,
            "",
            true, // higher is better
            None,
            None,
        ));
    }

    // Dead File % — lower is better
    if let (Some(prev_val), Some(cur_val)) =
        (prev.vital_signs.dead_file_pct, current_vs.dead_file_pct)
    {
        metrics.push(make_metric(
            "dead_file_pct",
            "Dead Files",
            prev_val,
            cur_val,
            "%",
            false,
            Some(TrendCount {
                value: prev.counts.dead_files,
                total: prev.counts.total_files,
            }),
            Some(TrendCount {
                value: current_counts.dead_files,
                total: current_counts.total_files,
            }),
        ));
    }

    // Dead Export % — lower is better
    if let (Some(prev_val), Some(cur_val)) =
        (prev.vital_signs.dead_export_pct, current_vs.dead_export_pct)
    {
        metrics.push(make_metric(
            "dead_export_pct",
            "Dead Exports",
            prev_val,
            cur_val,
            "%",
            false,
            Some(TrendCount {
                value: prev.counts.dead_exports,
                total: prev.counts.total_exports,
            }),
            Some(TrendCount {
                value: current_counts.dead_exports,
                total: current_counts.total_exports,
            }),
        ));
    }

    // Avg Cyclomatic — lower is better
    {
        metrics.push(make_metric(
            "avg_cyclomatic",
            "Avg Cyclomatic",
            prev.vital_signs.avg_cyclomatic,
            current_vs.avg_cyclomatic,
            "",
            false,
            None,
            None,
        ));
    }

    // Maintainability — higher is better
    if let (Some(prev_val), Some(cur_val)) = (
        prev.vital_signs.maintainability_avg,
        current_vs.maintainability_avg,
    ) {
        metrics.push(make_metric(
            "maintainability_avg",
            "Maintainability",
            prev_val,
            cur_val,
            "",
            true,
            None,
            None,
        ));
    }

    // Unused Deps — lower is better
    if let (Some(prev_val), Some(cur_val)) = (
        prev.vital_signs.unused_dep_count,
        current_vs.unused_dep_count,
    ) {
        metrics.push(make_metric(
            "unused_dep_count",
            "Unused Deps",
            f64::from(prev_val),
            f64::from(cur_val),
            "",
            false,
            None,
            None,
        ));
    }

    // Circular Deps — lower is better
    if let (Some(prev_val), Some(cur_val)) = (
        prev.vital_signs.circular_dep_count,
        current_vs.circular_dep_count,
    ) {
        metrics.push(make_metric(
            "circular_dep_count",
            "Circular Deps",
            f64::from(prev_val),
            f64::from(cur_val),
            "",
            false,
            None,
            None,
        ));
    }

    // Hotspot Count — lower is better
    if let (Some(prev_val), Some(cur_val)) =
        (prev.vital_signs.hotspot_count, current_vs.hotspot_count)
    {
        metrics.push(make_metric(
            "hotspot_count",
            "Hotspots",
            f64::from(prev_val),
            f64::from(cur_val),
            "",
            false,
            None,
            None,
        ));
    }

    // Unit size very-high-risk % — lower is better
    if let (Some(prev_profile), Some(cur_profile)) = (
        &prev.vital_signs.unit_size_profile,
        &current_vs.unit_size_profile,
    ) {
        metrics.push(make_metric(
            "unit_size_very_high_pct",
            "Oversized Fns",
            prev_profile.very_high_risk,
            cur_profile.very_high_risk,
            "%",
            false,
            None,
            None,
        ));
    }

    // P95 fan-in — lower is better
    if let (Some(prev_val), Some(cur_val)) = (prev.vital_signs.p95_fan_in, current_vs.p95_fan_in) {
        metrics.push(make_metric(
            "p95_fan_in",
            "P95 Fan-in",
            f64::from(prev_val),
            f64::from(cur_val),
            "",
            false,
            None,
            None,
        ));
    }

    // Duplication % — lower is better
    if let (Some(prev_val), Some(cur_val)) =
        (prev.vital_signs.duplication_pct, current_vs.duplication_pct)
    {
        metrics.push(make_metric(
            "duplication_pct",
            "Duplication",
            prev_val,
            cur_val,
            "%",
            false,
            prev.counts
                .duplicated_lines
                .zip(prev.counts.total_lines)
                .map(|(d, t)| TrendCount { value: d, total: t }),
            current_counts
                .duplicated_lines
                .zip(current_counts.total_lines)
                .map(|(d, t)| TrendCount { value: d, total: t }),
        ));
    }

    // Determine overall direction
    let (improving, declining) =
        metrics
            .iter()
            .fold((0usize, 0usize), |(imp, dec), m| match m.direction {
                TrendDirection::Improving => (imp + 1, dec),
                TrendDirection::Declining => (imp, dec + 1),
                TrendDirection::Stable => (imp, dec),
            });
    let overall_direction = match improving.cmp(&declining) {
        std::cmp::Ordering::Greater => TrendDirection::Improving,
        std::cmp::Ordering::Less => TrendDirection::Declining,
        std::cmp::Ordering::Equal => TrendDirection::Stable,
    };

    Some(HealthTrend {
        compared_to,
        metrics,
        snapshots_loaded: snapshots.len(),
        overall_direction,
    })
}

/// Build a single trend metric.
#[expect(
    clippy::too_many_arguments,
    reason = "metric builder needs all parameters"
)]
fn make_metric(
    name: &'static str,
    label: &'static str,
    previous: f64,
    current: f64,
    unit: &'static str,
    higher_is_better: bool,
    previous_count: Option<TrendCount>,
    current_count: Option<TrendCount>,
) -> TrendMetric {
    let delta = (current - previous).round_to(1);
    let direction = if delta.abs() < TREND_TOLERANCE {
        TrendDirection::Stable
    } else if (higher_is_better && delta > 0.0) || (!higher_is_better && delta < 0.0) {
        TrendDirection::Improving
    } else {
        TrendDirection::Declining
    };

    TrendMetric {
        name,
        label,
        previous,
        current,
        delta,
        direction,
        unit,
        previous_count,
        current_count,
    }
}

/// Extension trait for rounding floats to N decimal places.
trait RoundTo {
    fn round_to(self, decimals: u32) -> Self;
}

impl RoundTo for f64 {
    fn round_to(self, decimals: u32) -> Self {
        let factor = 10_f64.powi(decimals as i32);
        (self * factor).round() / factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module(id: u32, cyclomatic: u16) -> fallow_core::extract::ModuleInfo {
        fallow_core::extract::ModuleInfo {
            file_id: fallow_core::discover::FileId(id),
            exports: Vec::new(),
            imports: Vec::new(),
            re_exports: Vec::new(),
            dynamic_imports: Vec::new(),
            dynamic_import_patterns: Vec::new(),
            require_calls: Vec::new(),
            member_accesses: Vec::new(),
            whole_object_uses: Vec::new(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: Vec::new(),
            unknown_suppression_kinds: Vec::new(),
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: Vec::new(),
            flag_uses: Vec::new(),
            class_heritage: Vec::new(),
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            auto_import_candidates: Vec::new(),
            complexity: vec![fallow_types::extract::FunctionComplexity {
                name: format!("fn_{id}"),
                line: id + 1,
                col: 0,
                cyclomatic,
                cognitive: 0,
                line_count: 10,
                param_count: 0,
                source_hash: None,
            }],
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test values are trivially small"
    )]
    fn make_modules() -> Vec<fallow_core::extract::ModuleInfo> {
        // Cyclomatic values: 2, 4, 6, 8, 10, 12, 14, 16, 18, 20
        (0..10)
            .map(|i| make_module(i, (i as u16 + 1) * 2))
            .collect()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < f64::EPSILON,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_some_close(actual: Option<f64>, expected: f64) {
        assert_close(actual.expect("expected metric to be present"), expected);
    }

    #[test]
    fn compute_cyclomatic_stats() {
        let modules = make_modules();
        let input = VitalSignsInput {
            modules: &modules,
            module_filter: None,
            file_scores: None,
            hotspots: None,
            total_files: 10,
            analysis_counts: None,
        };
        let vs = compute_vital_signs(&input);
        // avg of 2,4,6,8,10,12,14,16,18,20 = 11.0
        assert!((vs.avg_cyclomatic - 11.0).abs() < f64::EPSILON);
        // p90 of sorted [2,4,6,8,10,12,14,16,18,20] at index ceil(10*0.9)-1 = 8 → value 18
        assert_eq!(vs.p90_cyclomatic, 18);
    }

    #[test]
    fn compute_with_analysis_counts() {
        let modules = make_modules();
        let input = VitalSignsInput {
            modules: &modules,
            module_filter: None,
            file_scores: None,
            hotspots: None,
            total_files: 100,
            analysis_counts: Some(AnalysisCounts {
                total_exports: 500,
                dead_files: 5,
                dead_exports: 50,
                unused_deps: 3,
                circular_deps: 2,
                total_deps: 40,
            }),
        };
        let vs = compute_vital_signs(&input);
        assert_eq!(vs.dead_file_pct, Some(5.0)); // 5/100 * 100
        assert_eq!(vs.dead_export_pct, Some(10.0)); // 50/500 * 100
        assert_eq!(vs.unused_dep_count, Some(3));
        assert_eq!(vs.circular_dep_count, Some(2));
    }

    #[test]
    fn compute_hotspot_count_with_threshold() {
        let hotspots = vec![
            HotspotEntry {
                path: PathBuf::from("a.ts"),
                score: 80.0,
                commits: 10,
                weighted_commits: 8.0,
                lines_added: 100,
                lines_deleted: 50,
                complexity_density: 0.5,
                fan_in: 5,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            },
            HotspotEntry {
                path: PathBuf::from("b.ts"),
                score: 30.0, // Below threshold
                commits: 5,
                weighted_commits: 3.0,
                lines_added: 40,
                lines_deleted: 20,
                complexity_density: 0.2,
                fan_in: 2,
                trend: fallow_core::churn::ChurnTrend::Cooling,
                ownership: None,
                is_test_path: false,
            },
            HotspotEntry {
                path: PathBuf::from("c.ts"),
                score: 50.0, // At threshold
                commits: 8,
                weighted_commits: 6.0,
                lines_added: 80,
                lines_deleted: 30,
                complexity_density: 0.4,
                fan_in: 3,
                trend: fallow_core::churn::ChurnTrend::Accelerating,
                ownership: None,
                is_test_path: false,
            },
        ];
        let modules = Vec::new();
        let input = VitalSignsInput {
            modules: &modules,
            module_filter: None,
            file_scores: None,
            hotspots: Some(&hotspots),
            total_files: 10,
            analysis_counts: None,
        };
        let vs = compute_vital_signs(&input);
        assert_eq!(vs.hotspot_count, Some(2)); // 80.0 and 50.0 meet threshold
        assert_eq!(vs.hotspot_top_pct_count, Some(1)); // top 1% bucket rounds up to one file
    }

    #[test]
    fn compute_without_hotspots_gives_none() {
        let modules = Vec::new();
        let input = VitalSignsInput {
            modules: &modules,
            module_filter: None,
            file_scores: None,
            hotspots: None,
            total_files: 0,
            analysis_counts: None,
        };
        let vs = compute_vital_signs(&input);
        assert!(vs.hotspot_count.is_none());
    }

    #[test]
    fn snapshot_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let vs = VitalSigns {
            dead_file_pct: Some(3.2),
            dead_export_pct: Some(8.1),
            avg_cyclomatic: 4.7,
            p90_cyclomatic: 12,
            hotspot_count: Some(5),
            maintainability_avg: Some(72.4),
            unused_dep_count: Some(4),
            circular_dep_count: Some(2),
            ..Default::default()
        };
        let counts = VitalSignsCounts {
            total_files: 1200,
            total_exports: 5400,
            dead_files: 38,
            dead_exports: 437,
            files_scored: Some(1150),
            total_deps: 42,
            ..Default::default()
        };
        let health_score = compute_health_score(&vs, 1200);
        let snapshot = build_snapshot(vs, counts, root, false, Some(&health_score), None);
        let saved_path = save_snapshot(&snapshot, root, None).unwrap();

        assert!(saved_path.exists());
        assert!(saved_path.starts_with(root.join(".fallow/snapshots")));

        // Load and verify
        let content = std::fs::read_to_string(&saved_path).unwrap();
        let loaded: VitalSignsSnapshot = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.snapshot_schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert!((loaded.vital_signs.avg_cyclomatic - 4.7).abs() < f64::EPSILON);
        assert_eq!(loaded.counts.total_files, 1200);
        assert!(loaded.score.is_some());
        assert!(loaded.grade.is_some());
    }

    #[test]
    fn snapshot_save_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let explicit = root.join("my-snapshot.json");
        let vs = VitalSigns {
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            ..Default::default()
        };
        let counts = VitalSignsCounts::default();
        let snapshot = build_snapshot(vs, counts, root, false, None, None);
        let saved = save_snapshot(&snapshot, root, Some(&explicit)).unwrap();
        assert_eq!(saved, explicit);
        assert!(explicit.exists());
    }

    #[test]
    fn snapshot_save_creates_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("a/b/c/snapshot.json");
        let vs = VitalSigns {
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            ..Default::default()
        };
        let counts = VitalSignsCounts::default();
        let snapshot = build_snapshot(vs, counts, root, false, None, None);
        let saved = save_snapshot(&snapshot, root, Some(&nested)).unwrap();
        assert_eq!(saved, nested);
        assert!(nested.exists());
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2026-03-25 is 20,537 days since epoch
        assert_eq!(days_to_ymd(20_537), (2026, 3, 25));
    }

    // --- compute_health_score ---

    #[test]
    fn health_score_perfect() {
        let vs = VitalSigns {
            dead_file_pct: Some(0.0),
            dead_export_pct: Some(0.0),
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            hotspot_count: Some(0),
            maintainability_avg: Some(90.0),
            unused_dep_count: Some(0),
            circular_dep_count: Some(0),
            ..Default::default()
        };
        let score = compute_health_score(&vs, 100);
        assert!((score.score - 100.0).abs() < f64::EPSILON);
        assert_eq!(score.grade, "A");
    }

    #[test]
    fn health_score_no_optional_metrics() {
        // Only avg_cyclomatic and p90_cyclomatic are always present
        let vs = VitalSigns {
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            ..Default::default()
        };
        let score = compute_health_score(&vs, 0);
        // Only complexity penalties apply (both 0 since below thresholds)
        assert!((score.score - 100.0).abs() < f64::EPSILON);
        assert_eq!(score.grade, "A");
        assert!(score.penalties.dead_files.is_none());
        assert!(score.penalties.unused_deps.is_none());
        assert!(score.penalties.duplication.is_none());
    }

    #[test]
    fn health_score_dead_code_penalty() {
        let vs = VitalSigns {
            dead_file_pct: Some(50.0),
            dead_export_pct: Some(30.0),
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            ..Default::default()
        };
        let score = compute_health_score(&vs, 100);
        // dead_file: min(50*0.2, 15) = 10
        // dead_export: min(30*0.2, 15) = 6
        // total penalty: 16
        assert!((score.score - 84.0).abs() < 0.1);
        assert_eq!(score.grade, "B");
    }

    #[test]
    fn health_score_complexity_penalty() {
        let vs = VitalSigns {
            avg_cyclomatic: 5.5,
            p90_cyclomatic: 15,
            ..Default::default()
        };
        let score = compute_health_score(&vs, 100);
        // complexity: min((5.5-1.5)*5, 20) = 20
        // p90: min(15-10, 10) = 5
        // total penalty: 25
        assert!((score.score - 75.0).abs() < 0.1);
        assert_eq!(score.grade, "B");
    }

    #[test]
    fn health_score_clamped_at_zero() {
        let vs = VitalSigns {
            dead_file_pct: Some(100.0),
            dead_export_pct: Some(100.0),
            avg_cyclomatic: 10.0,
            p90_cyclomatic: 30,
            hotspot_count: Some(50),
            maintainability_avg: Some(20.0),
            unused_dep_count: Some(100),
            circular_dep_count: Some(50),
            ..Default::default()
        };
        let score = compute_health_score(&vs, 100);
        assert!((score.score).abs() < f64::EPSILON);
        assert_eq!(score.grade, "F");
    }

    #[test]
    fn health_score_hotspot_normalized_by_files() {
        let vs = VitalSigns {
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            hotspot_count: Some(5),
            ..Default::default()
        };
        // 5 hotspots in 100 files = 5% = 10 points
        let score_100 = compute_health_score(&vs, 100);
        // 5 hotspots in 1000 files = 0.5% = 1 point
        let score_1000 = compute_health_score(&vs, 1000);
        assert!(score_1000.score > score_100.score);
    }

    #[test]
    fn health_score_hotspot_top_pct_can_use_full_budget() {
        let vs = VitalSigns {
            avg_cyclomatic: 1.0,
            p90_cyclomatic: 2,
            hotspot_count: Some(0),
            hotspot_top_pct_count: Some(250),
            ..Default::default()
        };

        let score = compute_health_score(&vs, 25_000);

        assert_some_close(score.penalties.hotspots, 10.0);
        assert_close(score.score, 90.0);
    }

    #[test]
    fn health_score_duplication_penalty() {
        let vs = VitalSigns {
            dead_file_pct: None,
            dead_export_pct: None,
            avg_cyclomatic: 1.0,
            critical_complexity_pct: None,
            p90_cyclomatic: 2,
            duplication_pct: Some(10.0), // 10% - 5% = 5 points
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
            total_loc: 0,
        };
        let score = compute_health_score(&vs, 100);
        assert_eq!(score.penalties.duplication, Some(5.0));

        // Below threshold: 4% duplication should not penalize
        let vs_low = VitalSigns {
            duplication_pct: Some(4.0),
            ..vs.clone()
        };
        let score_low = compute_health_score(&vs_low, 100);
        assert_eq!(score_low.penalties.duplication, Some(0.0));

        // At cap: 20% should cap at 10 points
        let vs_high = VitalSigns {
            duplication_pct: Some(20.0),
            ..vs
        };
        let score_high = compute_health_score(&vs_high, 100);
        assert_eq!(score_high.penalties.duplication, Some(10.0));
    }

    #[test]
    fn health_score_uses_scale_invariant_monorepo_signals() {
        let vs = VitalSigns {
            dead_file_pct: Some(4.0),
            dead_export_pct: Some(9.0),
            avg_cyclomatic: 2.3,
            critical_complexity_pct: Some(2.3),
            p90_cyclomatic: 4,
            duplication_pct: Some(6.0),
            hotspot_count: Some(0),
            hotspot_top_pct_count: Some(250),
            maintainability_avg: Some(91.0),
            maintainability_low_pct: Some(8.0),
            unused_dep_count: Some(180),
            unused_deps_per_k_files: Some(7.2),
            circular_dep_count: Some(450),
            circular_deps_per_k_files: Some(18.0),
            unit_size_profile: Some(RiskProfile {
                low_risk: 80.0,
                medium_risk: 12.7,
                high_risk: 5.0,
                very_high_risk: 2.3,
            }),
            functions_over_60_loc_per_k: Some(23.0),
            p95_fan_in: Some(7),
            coupling_high_pct: Some(4.0),
            ..Default::default()
        };
        let score = compute_health_score(&vs, 25_000);
        let penalties = &score.penalties;

        assert_some_close(penalties.dead_files, 0.8);
        assert_some_close(penalties.dead_exports, 1.8);
        assert_close(penalties.complexity, 9.2);
        assert!((penalties.p90_complexity).abs() < f64::EPSILON);
        assert_some_close(penalties.maintainability, 12.0);
        assert_some_close(penalties.hotspots, 10.0);
        assert_some_close(penalties.unused_deps, 3.6);
        assert_some_close(penalties.circular_deps, 9.0);
        assert_some_close(penalties.unit_size, 10.0);
        assert_some_close(penalties.coupling, 2.0);
        assert_some_close(penalties.duplication, 1.0);
        assert_close(score.score, 40.6);
        assert_eq!(score.grade, "D");
    }

    // --- load_snapshots ---

    #[test]
    fn load_snapshots_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let snaps = load_snapshots(dir.path());
        assert!(snaps.is_empty());
    }

    #[test]
    fn load_snapshots_returns_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let snap_dir = root.join(".fallow/snapshots");
        std::fs::create_dir_all(&snap_dir).unwrap();

        let older = make_test_snapshot("2026-01-01T00:00:00Z", Some(72.0));
        let newer = make_test_snapshot("2026-03-01T00:00:00Z", Some(78.0));

        // Write newer first to test sorting
        std::fs::write(
            snap_dir.join("2026-03-01T00-00-00Z.json"),
            serde_json::to_string(&newer).unwrap(),
        )
        .unwrap();
        std::fs::write(
            snap_dir.join("2026-01-01T00-00-00Z.json"),
            serde_json::to_string(&older).unwrap(),
        )
        .unwrap();

        let loaded = load_snapshots(root);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].timestamp, "2026-01-01T00:00:00Z");
        assert_eq!(loaded[1].timestamp, "2026-03-01T00:00:00Z");
    }

    #[test]
    fn load_snapshots_skips_corrupt_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let snap_dir = root.join(".fallow/snapshots");
        std::fs::create_dir_all(&snap_dir).unwrap();

        std::fs::write(snap_dir.join("corrupt.json"), "not valid json").unwrap();
        let good = make_test_snapshot("2026-02-01T00:00:00Z", Some(80.0));
        std::fs::write(
            snap_dir.join("good.json"),
            serde_json::to_string(&good).unwrap(),
        )
        .unwrap();

        let loaded = load_snapshots(root);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].timestamp, "2026-02-01T00:00:00Z");
    }

    #[test]
    fn load_snapshots_ignores_non_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let snap_dir = root.join(".fallow/snapshots");
        std::fs::create_dir_all(&snap_dir).unwrap();

        std::fs::write(snap_dir.join("readme.txt"), "not a snapshot").unwrap();

        let loaded = load_snapshots(root);
        assert!(loaded.is_empty());
    }

    // --- compute_trend ---

    #[test]
    fn compute_trend_no_snapshots() {
        let vs = make_test_vital_signs();
        let counts = make_test_counts();
        assert!(compute_trend(&vs, &counts, Some(78.0), &[]).is_none());
    }

    #[test]
    fn compute_trend_improving() {
        let prev = make_test_snapshot("2026-01-01T00:00:00Z", Some(72.0));
        let vs = VitalSigns {
            dead_file_pct: Some(2.8),
            dead_export_pct: Some(7.5),
            avg_cyclomatic: 4.1,
            p90_cyclomatic: 12,
            hotspot_count: Some(3),
            maintainability_avg: Some(75.0),
            unused_dep_count: Some(3),
            circular_dep_count: Some(1),
            ..Default::default()
        };
        let counts = VitalSignsCounts {
            total_files: 100,
            total_exports: 500,
            dead_files: 3,
            dead_exports: 38,
            files_scored: Some(95),
            total_deps: 40,
            ..Default::default()
        };

        let trend = compute_trend(&vs, &counts, Some(78.0), &[prev]).unwrap();
        assert_eq!(trend.compared_to.timestamp, "2026-01-01T00:00:00Z");
        assert_eq!(trend.snapshots_loaded, 1);
        assert_eq!(trend.overall_direction, TrendDirection::Improving);

        // Score should be improving (72 → 78)
        let score_metric = trend.metrics.iter().find(|m| m.name == "score").unwrap();
        assert_eq!(score_metric.direction, TrendDirection::Improving);
        assert!((score_metric.delta - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_trend_stable_within_tolerance() {
        let prev = make_test_snapshot("2026-01-01T00:00:00Z", Some(78.0));
        let vs = make_test_vital_signs();
        let counts = make_test_counts();

        let trend = compute_trend(&vs, &counts, Some(78.3), &[prev]).unwrap();
        let score_metric = trend.metrics.iter().find(|m| m.name == "score").unwrap();
        assert_eq!(score_metric.direction, TrendDirection::Stable);
    }

    #[test]
    fn compute_trend_uses_most_recent_snapshot() {
        let older = make_test_snapshot("2026-01-01T00:00:00Z", Some(60.0));
        let newer = make_test_snapshot("2026-03-01T00:00:00Z", Some(72.0));
        let vs = make_test_vital_signs();
        let counts = make_test_counts();

        let trend = compute_trend(&vs, &counts, Some(78.0), &[older, newer]).unwrap();
        // Should compare against newer (72.0), not older (60.0)
        assert_eq!(trend.compared_to.score, Some(72.0));
        assert_eq!(trend.snapshots_loaded, 2);
    }

    #[test]
    fn compute_trend_includes_raw_counts() {
        let prev = make_test_snapshot("2026-01-01T00:00:00Z", Some(72.0));
        let vs = make_test_vital_signs();
        let counts = make_test_counts();

        let trend = compute_trend(&vs, &counts, Some(78.0), &[prev]).unwrap();
        let dead_files = trend
            .metrics
            .iter()
            .find(|m| m.name == "dead_file_pct")
            .unwrap();
        assert!(dead_files.previous_count.is_some());
        assert!(dead_files.current_count.is_some());
    }

    // --- test helpers ---

    fn make_test_vital_signs() -> VitalSigns {
        VitalSigns {
            dead_file_pct: Some(3.2),
            dead_export_pct: Some(8.1),
            avg_cyclomatic: 4.2,
            p90_cyclomatic: 12,
            hotspot_count: Some(5),
            maintainability_avg: Some(72.4),
            unused_dep_count: Some(4),
            circular_dep_count: Some(2),
            ..Default::default()
        }
    }

    fn make_test_counts() -> VitalSignsCounts {
        VitalSignsCounts {
            total_files: 100,
            total_exports: 500,
            dead_files: 3,
            dead_exports: 40,
            files_scored: Some(95),
            total_deps: 42,
            ..Default::default()
        }
    }

    fn make_test_snapshot(timestamp: &str, score: Option<f64>) -> VitalSignsSnapshot {
        VitalSignsSnapshot {
            snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
            version: "2.5.5".into(),
            timestamp: timestamp.into(),
            git_sha: Some("abc1234".into()),
            git_branch: Some("main".into()),
            shallow_clone: false,
            vital_signs: VitalSigns {
                dead_file_pct: Some(3.2),
                dead_export_pct: Some(8.1),
                avg_cyclomatic: 4.7,
                p90_cyclomatic: 12,
                hotspot_count: Some(5),
                maintainability_avg: Some(72.4),
                unused_dep_count: Some(4),
                circular_dep_count: Some(2),
                ..Default::default()
            },
            counts: VitalSignsCounts {
                total_files: 100,
                total_exports: 500,
                dead_files: 3,
                dead_exports: 40,
                files_scored: Some(95),
                total_deps: 42,
                ..Default::default()
            },
            score,
            grade: score.map(|s| letter_grade(s).to_string()),
            coverage_model: None,
        }
    }
}
