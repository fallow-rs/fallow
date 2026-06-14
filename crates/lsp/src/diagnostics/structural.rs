use rustc_hash::FxHashMap;

use ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Uri,
};

use fallow_core::results::AnalysisResults;

use super::{FIRST_LINE_RANGE, doc_link};

/// Basename of `path`, falling back to the full display string.
fn cycle_file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Stable identifier shared by every per-file diagnostic of one cycle, so
/// editors / agents can fold the N squigglies into a single "one cycle shown
/// N times" concept. FNV-1a over the sorted file paths, so the id is
/// independent of which file the cycle is rotated to start at.
fn cycle_fingerprint(files: &[std::path::PathBuf]) -> String {
    let mut sorted: Vec<String> = files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    sorted.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for entry in &sorted {
        for byte in entry.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= u64::from(b'\n');
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("cycle:{hash:016x}")
}

/// Legacy single-diagnostic emission for cycles whose data carries no
/// per-file `edges` anchors (historical baseline JSON, or test fixtures that
/// build `CircularDependency` without populating `edges`). One diagnostic on
/// the first file, with the other members listed as related info at line 0.
fn push_legacy_circular_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    cycle: &fallow_core::results::CircularDependency,
    names: &[String],
) {
    let Some(first_file) = cycle.files.first() else {
        return;
    };
    let Some(uri) = Uri::from_file_path(first_file) else {
        return;
    };
    let message = format!("Circular dependency: {}", names.join(" \u{2192} "));
    let line = cycle.line.saturating_sub(1);

    let related_info: Vec<DiagnosticRelatedInformation> = cycle
        .files
        .iter()
        .skip(1)
        .enumerate()
        .filter_map(|(i, f)| {
            let file_uri = Uri::from_file_path(f)?;
            Some(DiagnosticRelatedInformation {
                location: Location {
                    uri: file_uri,
                    range: FIRST_LINE_RANGE,
                },
                message: format!("Step {} in cycle: {}", i + 2, cycle_file_name(f)),
            })
        })
        .collect();

    map.entry(uri).or_default().push(Diagnostic {
        range: Range {
            start: Position {
                line,
                character: cycle.col,
            },
            end: Position {
                line,
                character: u32::MAX,
            },
        },
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String("circular-dependency".to_string())),
        code_description: doc_link("circular-dependencies"),
        message,
        related_information: if related_info.is_empty() {
            None
        } else {
            Some(related_info)
        },
        ..Default::default()
    });
}

pub fn push_circular_dep_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for cycle in &results.circular_dependencies {
        let files = &cycle.cycle.files;
        if files.is_empty() {
            continue;
        }
        // No per-file anchors (old data): fall back to the single-first-file
        // diagnostic so behavior is unchanged for consumers predating `edges`.
        if cycle.cycle.edges.is_empty() {
            let file_names: Vec<String> = files.iter().map(|f| cycle_file_name(f)).collect();
            push_legacy_circular_diagnostic(map, &cycle.cycle, &file_names);
            continue;
        }

        // Names are derived from the EDGES (not `files`) so all the rotated
        // message and related-info index math below stays in bounds even if a
        // caller ever passes `edges.len() != files.len()`. Core enforces the
        // invariant; the LSP renders without depending on it.
        let names: Vec<String> = cycle
            .cycle
            .edges
            .iter()
            .map(|edge| cycle_file_name(&edge.path))
            .collect();
        let n = names.len();
        let cycle_id = cycle_fingerprint(files);
        let suffix = if n == 1 { "" } else { "s" };

        for (i, edge) in cycle.cycle.edges.iter().enumerate() {
            let Some(uri) = Uri::from_file_path(&edge.path) else {
                // Render-only drop: an unopenable URL (e.g. a relative or
                // malformed path) is skipped here, but the `edges` data still
                // carries every hop. Never let this filter touch the data.
                continue;
            };
            let line = edge.line.saturating_sub(1);
            // Rotate the chain so the message reads from the file the user is
            // standing in: on `b` of a -> b -> c -> a it reads
            // "Circular dependency (3 files): b -> c -> a -> b".
            let rotated: Vec<&str> = (0..=n).map(|k| names[(i + k) % n].as_str()).collect();
            let message = format!(
                "Circular dependency ({n} file{suffix}): {}",
                rotated.join(" \u{2192} "),
            );

            let related_info: Vec<DiagnosticRelatedInformation> =
                cycle
                    .cycle
                    .edges
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .filter_map(|(j, other)| {
                        let other_uri = Uri::from_file_path(&other.path)?;
                        let other_line = other.line.saturating_sub(1);
                        Some(DiagnosticRelatedInformation {
                            location: Location {
                                uri: other_uri,
                                range: Range {
                                    start: Position {
                                        line: other_line,
                                        character: other.col,
                                    },
                                    end: Position {
                                        line: other_line,
                                        character: u32::MAX,
                                    },
                                },
                            },
                            message: format!(
                                "Cycle hop: {} \u{2192} {}",
                                names[j],
                                names[(j + 1) % n],
                            ),
                        })
                    })
                    .collect();

            map.entry(uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: edge.col,
                    },
                    end: Position {
                        line,
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("circular-dependency".to_string())),
                code_description: doc_link("circular-dependencies"),
                message,
                related_information: if related_info.is_empty() {
                    None
                } else {
                    Some(related_info)
                },
                // Shared cycle identity so editors / agents can correlate the
                // N per-file squigglies into one cycle. `attach_changed_since_data`
                // merges `changedSince` into this object without clobbering it.
                data: Some(serde_json::json!({
                    "circularDependency": { "cycleId": cycle_id, "fileCount": n }
                })),
                ..Default::default()
            });
        }
    }
}

pub fn push_re_export_cycle_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for cycle in &results.re_export_cycles {
        let chain: Vec<String> = cycle
            .cycle
            .files
            .iter()
            .map(|f| {
                f.file_name().map_or_else(
                    || f.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            })
            .collect();
        let (kind_label, fix_hint) = match cycle.cycle.kind {
            fallow_core::results::ReExportCycleKind::SelfLoop => (
                "Self-loop",
                "Remove the `export * from './'` (or equivalent) inside this file.",
            ),
            fallow_core::results::ReExportCycleKind::MultiNode => (
                "Cycle",
                "Remove one `export * from` statement on any one member to break the cycle.",
            ),
        };
        let message = format!(
            "Re-export {} ({} file{}): {}. {}",
            kind_label.to_ascii_lowercase(),
            cycle.cycle.files.len(),
            if cycle.cycle.files.len() == 1 {
                ""
            } else {
                "s"
            },
            chain.join(" <-> "),
            fix_hint
        );

        for (idx, member_path) in cycle.cycle.files.iter().enumerate() {
            let Some(uri) = Uri::from_file_path(member_path) else {
                continue;
            };
            let related_info: Vec<DiagnosticRelatedInformation> = cycle
                .cycle
                .files
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .filter_map(|(_, other)| {
                    let other_uri = Uri::from_file_path(other)?;
                    let name = other.file_name().map_or_else(
                        || other.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    Some(DiagnosticRelatedInformation {
                        location: Location {
                            uri: other_uri,
                            range: FIRST_LINE_RANGE,
                        },
                        message: format!("Other member: {name}"),
                    })
                })
                .collect();

            map.entry(uri).or_default().push(Diagnostic {
                range: FIRST_LINE_RANGE,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("re-export-cycle".to_string())),
                code_description: doc_link("re-export-cycles"),
                message: message.clone(),
                related_information: if related_info.is_empty() {
                    None
                } else {
                    Some(related_info)
                },
                ..Default::default()
            });
        }
    }
}

pub fn push_boundary_violation_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for v in &results.boundary_violations {
        let Some(uri) = Uri::from_file_path(&v.violation.from_path) else {
            continue;
        };
        let line = v.violation.line.saturating_sub(1);
        let to_name = v.violation.to_path.file_name().map_or_else(
            || v.violation.to_path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let message = format!(
            "Boundary violation: import of {} (zone '{}') is not allowed from zone '{}'",
            to_name, v.violation.to_zone, v.violation.from_zone,
        );

        let related_info = Uri::from_file_path(&v.violation.to_path).map(|target_uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri: target_uri,
                    range: FIRST_LINE_RANGE,
                },
                message: format!("Target file in zone '{}'", v.violation.to_zone),
            }]
        });

        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: v.violation.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("boundary-violation".to_string())),
            code_description: doc_link("boundary-violations"),
            message,
            related_information: related_info,
            ..Default::default()
        });
    }

    for v in &results.boundary_coverage_violations {
        let Some(uri) = Uri::from_file_path(&v.violation.path) else {
            continue;
        };
        let line = v.violation.line.saturating_sub(1);
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: v.violation.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("boundary-violation".to_string())),
            code_description: doc_link("boundary-violations"),
            message: "Boundary coverage: file does not match any configured zone".to_string(),
            related_information: None,
            ..Default::default()
        });
    }

    for v in &results.boundary_call_violations {
        let Some(uri) = Uri::from_file_path(&v.violation.path) else {
            continue;
        };
        let line = v.violation.line.saturating_sub(1);
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: v.violation.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("boundary-violation".to_string())),
            code_description: doc_link("boundary-violations"),
            message: format!(
                "Boundary call: `{}` matches forbidden pattern `{}` in zone '{}'",
                v.violation.callee, v.violation.pattern, v.violation.zone
            ),
            related_information: None,
            ..Default::default()
        });
    }
}

/// Surface rule-pack policy violations. Severity maps from the EFFECTIVE
/// per-finding severity (rule-level `severity` over the `policy-violation`
/// master), so an error-severity rule renders as an error squiggle while a
/// warn rule renders as a warning. Paths are absolute internally, so the URI
/// is built directly (no `root.join`).
pub fn push_policy_violation_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    use fallow_core::results::PolicyViolationSeverity;

    for v in &results.policy_violations {
        let Some(uri) = Uri::from_file_path(&v.violation.path) else {
            continue;
        };
        let line = v.violation.line.saturating_sub(1);
        let severity = match v.violation.severity {
            PolicyViolationSeverity::Error => DiagnosticSeverity::ERROR,
            PolicyViolationSeverity::Warn => DiagnosticSeverity::WARNING,
        };
        let message = match &v.violation.message {
            Some(message) => format!(
                "Policy violation: `{}` is banned by `{}/{}`. {message}",
                v.violation.matched, v.violation.pack, v.violation.rule_id
            ),
            None => format!(
                "Policy violation: `{}` is banned by `{}/{}`",
                v.violation.matched, v.violation.pack, v.violation.rule_id
            ),
        };
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: v.violation.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(severity),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("policy-violation".to_string())),
            code_description: doc_link("policy-violations"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

/// Push diagnostics for `"use client"` files that export a Next.js
/// server-only / route-config name. Fixed `WARNING` severity (the rule's
/// default), code `invalid-client-export`. Paths are absolute internally, so
/// the URI is built directly (no `root.join`).
pub fn push_invalid_client_export_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.invalid_client_exports {
        let Some(uri) = Uri::from_file_path(&finding.export.path) else {
            continue;
        };
        let line = finding.export.line.saturating_sub(1);
        let message = format!(
            "Export `{}` is not allowed in a \"{}\" file (Next.js server-only / route-config name)",
            finding.export.export_name, finding.export.directive
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: finding.export.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("invalid-client-export".to_string())),
            code_description: doc_link("invalid-client-exports"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

/// Push diagnostics for barrel files that re-export both a `"use client"`
/// origin and a server-only origin. Fixed `WARNING` severity (the rule's
/// default), code `mixed-client-server-barrel`. Paths are absolute internally,
/// so the URI is built directly (no `root.join`).
pub fn push_mixed_client_server_barrel_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.mixed_client_server_barrels {
        let Some(uri) = Uri::from_file_path(&finding.barrel.path) else {
            continue;
        };
        let line = finding.barrel.line.saturating_sub(1);
        let message = format!(
            "Barrel re-exports both a \"use client\" module (`{}`) and a server-only module (`{}`); one import drags the other's directive across the boundary",
            finding.barrel.client_origin, finding.barrel.server_origin
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: finding.barrel.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "mixed-client-server-barrel".to_string(),
            )),
            code_description: doc_link("mixed-client-server-barrels"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

/// Push diagnostics for misplaced `"use client"` / `"use server"`
/// directives. Fixed `WARNING` severity (the rule's default), code
/// `misplaced-directive`. Paths are absolute internally, so the URI is built
/// directly (no `root.join`).
pub fn push_misplaced_directive_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.misplaced_directives {
        let Some(uri) = Uri::from_file_path(&finding.directive_site.path) else {
            continue;
        };
        let line = finding.directive_site.line.saturating_sub(1);
        let message = format!(
            "Directive \"{}\" is not in the leading position, so the RSC bundler ignores it; move it to the top of the file",
            finding.directive_site.directive
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: finding.directive_site.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("misplaced-directive".to_string())),
            code_description: doc_link("misplaced-directives"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

/// Push diagnostics for `inject(KEY)` / `getContext(KEY)` calls that have no
/// matching `provide(KEY)` ancestor. Fixed `WARNING` severity (the rule's
/// default), code `unprovided-inject`. Paths are absolute internally, so the
/// URI is built directly (no `root.join`).
pub fn push_unprovided_inject_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.unprovided_injects {
        let Some(uri) = Uri::from_file_path(&finding.inject.path) else {
            continue;
        };
        let line = finding.inject.line.saturating_sub(1);
        let message = format!(
            "inject(`{}`) has no matching provide(`{}`); at runtime it returns undefined",
            finding.inject.key_name, finding.inject.key_name
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: finding.inject.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("unprovided-inject".to_string())),
            code_description: doc_link("unprovided-injects"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

/// Push `route-collision` diagnostics. File-level findings anchored at line 1;
/// the finding's `path` is the absolute graph-node path, so the URI is built
/// directly with no `root.join`.
pub fn push_route_collision_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.route_collisions {
        let Some(uri) = Uri::from_file_path(&finding.collision.path) else {
            continue;
        };
        let message = format!(
            "Route file resolves to '{}', also owned by {} other file(s); Next.js fails the build because a URL can have only one owner",
            finding.collision.url,
            finding.collision.conflicting_paths.len()
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("route-collision".to_string())),
            code_description: doc_link("route-collisions"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

/// Push `dynamic-segment-name-conflict` diagnostics. File-level findings
/// anchored at line 1; absolute `path`, so the URI is built directly.
pub fn push_dynamic_segment_name_conflict_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.dynamic_segment_name_conflicts {
        let Some(uri) = Uri::from_file_path(&finding.conflict.path) else {
            continue;
        };
        let message = format!(
            "Dynamic segments at '{}' use different slug names ({}); Next.js requires one consistent name per dynamic path",
            finding.conflict.position,
            finding.conflict.conflicting_segments.join(", ")
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "dynamic-segment-name-conflict".to_string(),
            )),
            code_description: doc_link("dynamic-segment-name-conflicts"),
            message,
            related_information: None,
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_core::duplicates::{DuplicationReport, DuplicationStats};
    use fallow_core::results::{
        AnalysisResults, BoundaryViolation, BoundaryViolationFinding, CircularDependency,
        CircularDependencyEdge, CircularDependencyFinding,
    };
    use ls_types::{DiagnosticSeverity, NumberOrString, Uri};

    use crate::diagnostics::build_diagnostics;

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn empty_duplication() -> DuplicationReport {
        DuplicationReport {
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
                clone_groups_below_min_occurrences: 0,
            },
        }
    }

    #[test]
    fn circular_dependency_produces_warning_with_chain_message() {
        let root = test_root();
        let file_a = root.join("src/a.ts");
        let file_b = root.join("src/b.ts");
        let file_c = root.join("src/c.ts");

        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![file_a.clone(), file_b.clone(), file_c.clone()],
                    length: 3,
                    line: 2,
                    col: 20,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri_a = Uri::from_file_path(&file_a).unwrap();
        let file_diags = &diags[&uri_a];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("circular-dependency".to_string()))
        );
        assert!(d.message.contains("Circular dependency"));
        assert!(d.message.contains("a.ts"));
        assert!(d.message.contains("b.ts"));
        assert!(d.message.contains("c.ts"));
        assert!(d.message.contains("\u{2192}")); // arrow separator

        assert_eq!(d.range.start.line, 1); // 1-based 2 -> 0-based 1
        assert_eq!(d.range.start.character, 20);
        assert_eq!(d.range.end.character, u32::MAX);

        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 2); // file_b and file_c (skips first file)
        assert_eq!(related[0].message, "Step 2 in cycle: b.ts");
        assert_eq!(related[1].message, "Step 3 in cycle: c.ts");

        let uri_b = Uri::from_file_path(&file_b).unwrap();
        let uri_c = Uri::from_file_path(&file_c).unwrap();
        assert_eq!(related[0].location.uri, uri_b);
        assert_eq!(related[1].location.uri, uri_c);
    }

    #[test]
    fn circular_dependency_with_single_file_has_no_related_info() {
        let root = test_root();
        let file_a = root.join("src/self.ts");

        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![file_a.clone()],
                    length: 1,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file_a).unwrap();
        let d = &diags[&uri][0];
        assert!(d.related_information.is_none());
    }

    #[test]
    fn circular_dependency_with_empty_files_produces_no_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![],
                    length: 0,
                    line: 0,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);
        assert!(diags.is_empty());
    }

    #[test]
    fn circular_dependency_with_edges_emits_per_file_diagnostics() {
        let root = test_root();
        let file_a = root.join("src/a.ts");
        let file_b = root.join("src/b.ts");
        let file_c = root.join("src/c.ts");

        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![file_a.clone(), file_b.clone(), file_c.clone()],
                    length: 3,
                    line: 5,
                    col: 8,
                    edges: vec![
                        CircularDependencyEdge {
                            path: file_a.clone(),
                            line: 5,
                            col: 8,
                        },
                        CircularDependencyEdge {
                            path: file_b.clone(),
                            line: 3,
                            col: 4,
                        },
                        CircularDependencyEdge {
                            path: file_c.clone(),
                            line: 7,
                            col: 2,
                        },
                    ],
                    is_cross_package: false,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri_a = Uri::from_file_path(&file_a).unwrap();
        let uri_b = Uri::from_file_path(&file_b).unwrap();
        let uri_c = Uri::from_file_path(&file_c).unwrap();

        // One squiggly per file in the cycle, each anchored at that file's
        // outgoing import.
        assert_eq!(diags[&uri_a].len(), 1);
        assert_eq!(diags[&uri_b].len(), 1);
        assert_eq!(diags[&uri_c].len(), 1);

        let da = &diags[&uri_a][0];
        assert_eq!(da.range.start.line, 4); // 1-based 5 -> 0-based 4
        assert_eq!(da.range.start.character, 8);
        assert_eq!(
            da.code,
            Some(NumberOrString::String("circular-dependency".to_string()))
        );
        // Message rotates to start at the current file.
        assert_eq!(
            da.message,
            "Circular dependency (3 files): a.ts \u{2192} b.ts \u{2192} c.ts \u{2192} a.ts"
        );

        let db = &diags[&uri_b][0];
        assert_eq!(db.range.start.line, 2);
        assert_eq!(db.range.start.character, 4);
        assert_eq!(
            db.message,
            "Circular dependency (3 files): b.ts \u{2192} c.ts \u{2192} a.ts \u{2192} b.ts"
        );

        let dc = &diags[&uri_c][0];
        assert_eq!(dc.range.start.line, 6);
        assert_eq!(dc.range.start.character, 2);

        // related_information points at the OTHER hops' REAL locations, not
        // line 0.
        let related = da.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 2);
        let b_related = related
            .iter()
            .find(|r| r.location.uri == uri_b)
            .expect("file_b should be a related hop");
        assert_eq!(b_related.location.range.start.line, 2); // edge_b line 3 -> 0-based 2
        assert_eq!(b_related.location.range.start.character, 4);

        // Every per-file diagnostic shares one cycleId so editors / agents can
        // fold them into a single cycle; fileCount reflects the cycle size.
        let id_a = da.data.as_ref().unwrap()["circularDependency"]["cycleId"]
            .as_str()
            .unwrap();
        let id_b = db.data.as_ref().unwrap()["circularDependency"]["cycleId"]
            .as_str()
            .unwrap();
        let id_c = dc.data.as_ref().unwrap()["circularDependency"]["cycleId"]
            .as_str()
            .unwrap();
        assert_eq!(id_a, id_b);
        assert_eq!(id_b, id_c);
        assert!(id_a.starts_with("cycle:"));
        assert_eq!(
            da.data.as_ref().unwrap()["circularDependency"]["fileCount"],
            serde_json::json!(3)
        );
    }

    #[test]
    fn circular_dependency_edge_with_unopenable_path_is_dropped_from_render_only() {
        // An edge whose path is not an absolute file path cannot become a
        // file URI, so it gets no squiggly, but the OTHER hops still render.
        // This proves the render-side filter never short-circuits the loop.
        let root = test_root();
        let file_a = root.join("src/a.ts");
        let relative = PathBuf::from("relative/b.ts");

        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![file_a.clone(), relative.clone()],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: vec![
                        CircularDependencyEdge {
                            path: file_a.clone(),
                            line: 2,
                            col: 0,
                        },
                        CircularDependencyEdge {
                            path: relative.clone(),
                            line: 4,
                            col: 0,
                        },
                    ],
                    is_cross_package: false,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        // file_a (absolute) still renders; the relative hop is silently
        // skipped from rendering only.
        let uri_a = Uri::from_file_path(&file_a).unwrap();
        assert_eq!(diags[&uri_a].len(), 1);
        assert!(Uri::from_file_path(&relative).is_none());
    }

    #[test]
    fn re_export_cycle_multi_node_emits_one_diagnostic_per_member() {
        use fallow_core::results::{ReExportCycle, ReExportCycleFinding, ReExportCycleKind};

        let root = test_root();
        let file_a = root.join("src/api/index.ts");
        let file_b = root.join("src/api/internal/index.ts");

        let mut results = AnalysisResults::default();
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![file_a.clone(), file_b.clone()],
                kind: ReExportCycleKind::MultiNode,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri_a = Uri::from_file_path(&file_a).unwrap();
        let uri_b = Uri::from_file_path(&file_b).unwrap();
        assert_eq!(diags[&uri_a].len(), 1);
        assert_eq!(diags[&uri_b].len(), 1);

        let d = &diags[&uri_a][0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("re-export-cycle".to_string()))
        );
        assert!(d.message.contains("Re-export cycle"));
        assert!(d.message.contains("2 files"));
        assert!(d.message.contains("<->"));
        assert!(
            d.message
                .contains("Remove one `export * from` statement on any one member"),
            "multi-node message must carry the fix hint"
        );

        assert_eq!(d.range.start.line, 0);
        assert_eq!(d.range.start.character, 0);

        let href = d
            .code_description
            .as_ref()
            .expect("docs link should be present")
            .href
            .as_str();
        assert!(
            href.ends_with("#re-export-cycles"),
            "expected docs anchor in helpUri, got {href}"
        );

        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].location.uri, uri_b);
        assert!(related[0].message.contains("Other member"));
    }

    #[test]
    fn re_export_cycle_self_loop_emits_self_loop_message_and_no_related_info() {
        use fallow_core::results::{ReExportCycle, ReExportCycleFinding, ReExportCycleKind};

        let root = test_root();
        let file = root.join("src/utils/index.ts");

        let mut results = AnalysisResults::default();
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![file.clone()],
                kind: ReExportCycleKind::SelfLoop,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file).unwrap();
        let d = &diags[&uri][0];
        assert!(d.message.contains("Re-export self-loop"));
        assert!(d.message.contains("1 file"));
        assert!(!d.message.contains("1 files"), "self-loop must singularize");
        assert!(
            d.message.contains("Remove the `export * from './'`"),
            "self-loop message must carry the self-loop fix hint"
        );
        assert!(d.related_information.is_none());
    }

    #[test]
    fn boundary_violation_produces_warning_with_zone_message() {
        let root = test_root();
        let from_file = root.join("src/feature/api.ts");
        let to_file = root.join("src/core/secret.ts");

        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: from_file.clone(),
                to_path: to_file,
                from_zone: "feature".to_string(),
                to_zone: "core".to_string(),
                import_specifier: "../core/secret".to_string(),
                line: 3,
                col: 10,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&from_file).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("boundary-violation".to_string()))
        );
        assert!(d.message.contains("Boundary violation"));
        assert!(d.message.contains("secret.ts"));
        assert!(d.message.contains("core"));
        assert!(d.message.contains("feature"));

        assert_eq!(d.range.start.line, 2); // 1-based 3 -> 0-based 2
        assert_eq!(d.range.start.character, 10);
        assert_eq!(d.range.end.character, u32::MAX);
    }

    #[test]
    fn boundary_call_violation_produces_warning_under_expected_uri() {
        let root = test_root();
        let file = root.join("src/domain/policy.ts");

        let mut results = AnalysisResults::default();
        results.boundary_call_violations.push(
            fallow_core::results::BoundaryCallViolationFinding::with_actions(
                fallow_core::results::BoundaryCallViolation {
                    path: file.clone(),
                    line: 5,
                    col: 2,
                    zone: "domain".to_string(),
                    callee: "execSync".to_string(),
                    pattern: "child_process.*".to_string(),
                },
            ),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("boundary call diagnostic should land under the file URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("boundary-violation".to_string()))
        );
        assert!(d.message.contains("Boundary call"));
        assert!(d.message.contains("execSync"));
        assert!(d.message.contains("child_process.*"));
        assert!(d.message.contains("domain"));
        assert_eq!(d.range.start.line, 4); // 1-based 5 -> 0-based 4
        assert_eq!(d.range.start.character, 2);
    }

    #[test]
    fn policy_violation_produces_diagnostic_with_per_finding_severity() {
        let root = test_root();
        let file = root.join("src/app.ts");

        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(fallow_core::results::PolicyViolationFinding::with_actions(
                fallow_core::results::PolicyViolation {
                    path: file.clone(),
                    line: 7,
                    col: 2,
                    pack: "team-policy".to_string(),
                    rule_id: "no-moment".to_string(),
                    kind: fallow_core::results::PolicyRuleKind::BannedImport,
                    matched: "moment".to_string(),
                    severity: fallow_core::results::PolicyViolationSeverity::Error,
                    message: Some("Use date-fns.".to_string()),
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("policy diagnostic should land under the file URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("policy-violation".to_string()))
        );
        assert!(d.message.contains("team-policy/no-moment"));
        assert!(d.message.contains("Use date-fns."));
        assert_eq!(d.range.start.line, 6); // 1-based 7 -> 0-based 6
        assert_eq!(d.range.start.character, 2);
    }

    #[test]
    fn invalid_client_export_produces_warning_diagnostic() {
        let root = test_root();
        let file = root.join("app/page.tsx");

        let mut results = AnalysisResults::default();
        results.invalid_client_exports.push(
            fallow_core::results::InvalidClientExportFinding::with_actions(
                fallow_core::results::InvalidClientExport {
                    path: file.clone(),
                    export_name: "metadata".to_string(),
                    directive: "use client".to_string(),
                    line: 4,
                    col: 0,
                },
            ),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("invalid-client-export diagnostic should land under the file URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("invalid-client-export".to_string()))
        );
        assert!(d.message.contains("metadata"));
        assert!(d.message.contains("use client"));
        assert_eq!(d.range.start.line, 3); // 1-based 4 -> 0-based 3
        assert_eq!(d.range.start.character, 0);
    }

    #[test]
    fn mixed_client_server_barrel_produces_warning_diagnostic() {
        let root = test_root();
        let file = root.join("app/components/index.ts");

        let mut results = AnalysisResults::default();
        results.mixed_client_server_barrels.push(
            fallow_core::results::MixedClientServerBarrelFinding::with_actions(
                fallow_core::results::MixedClientServerBarrel {
                    path: file.clone(),
                    client_origin: "./Button".to_string(),
                    server_origin: "./fetchUser".to_string(),
                    line: 2,
                    col: 0,
                },
            ),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("mixed-client-server-barrel diagnostic should land under the file URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String(
                "mixed-client-server-barrel".to_string()
            ))
        );
        assert!(d.message.contains("./Button"));
        assert!(d.message.contains("./fetchUser"));
        assert_eq!(d.range.start.line, 1); // 1-based 2 -> 0-based 1
        assert_eq!(d.range.start.character, 0);
    }

    #[test]
    fn misplaced_directive_produces_warning_diagnostic() {
        let root = test_root();
        let file = root.join("app/page.tsx");

        let mut results = AnalysisResults::default();
        results.misplaced_directives.push(
            fallow_core::results::MisplacedDirectiveFinding::with_actions(
                fallow_core::results::MisplacedDirective {
                    path: file.clone(),
                    directive: "use client".to_string(),
                    line: 3,
                    col: 0,
                },
            ),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&file).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("misplaced-directive diagnostic should land under the file URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("misplaced-directive".to_string()))
        );
        assert!(d.message.contains("use client"));
        assert_eq!(d.range.start.line, 2); // 1-based 3 -> 0-based 2
        assert_eq!(d.range.start.character, 0);
    }

    #[test]
    fn boundary_violation_has_warning_severity() {
        let root = test_root();
        let from_file = root.join("src/ui/button.ts");
        let to_file = root.join("src/infra/db.ts");

        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: from_file.clone(),
                to_path: to_file,
                from_zone: "ui".to_string(),
                to_zone: "infra".to_string(),
                import_specifier: "../infra/db".to_string(),
                line: 1,
                col: 0,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&from_file).unwrap();
        let d = &diags[&uri][0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.source, Some("fallow".to_string()));
    }

    #[test]
    fn boundary_violation_has_related_info_linking_to_target() {
        let root = test_root();
        let from_file = root.join("src/app/page.ts");
        let to_file = root.join("src/domain/entity.ts");

        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: from_file.clone(),
                to_path: to_file.clone(),
                from_zone: "app".to_string(),
                to_zone: "domain".to_string(),
                import_specifier: "../domain/entity".to_string(),
                line: 5,
                col: 0,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&from_file).unwrap();
        let d = &diags[&uri][0];

        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, "Target file in zone 'domain'");

        let target_uri = Uri::from_file_path(&to_file).unwrap();
        assert_eq!(related[0].location.uri, target_uri);
    }

    #[test]
    fn multiple_boundary_violations_in_same_file_aggregate() {
        let root = test_root();
        let from_file = root.join("src/feature/handler.ts");
        let to_file_a = root.join("src/core/auth.ts");
        let to_file_b = root.join("src/infra/cache.ts");

        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: from_file.clone(),
                to_path: to_file_a,
                from_zone: "feature".to_string(),
                to_zone: "core".to_string(),
                import_specifier: "../core/auth".to_string(),
                line: 1,
                col: 0,
            }));
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: from_file.clone(),
                to_path: to_file_b,
                from_zone: "feature".to_string(),
                to_zone: "infra".to_string(),
                import_specifier: "../infra/cache".to_string(),
                line: 2,
                col: 0,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Uri::from_file_path(&from_file).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 2);

        assert!(file_diags[0].message.contains("auth.ts"));
        assert!(file_diags[1].message.contains("cache.ts"));
    }

    #[test]
    fn empty_boundary_violations_produces_no_diagnostics() {
        let root = test_root();
        let results = AnalysisResults::default();

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);
        assert!(diags.is_empty());
    }
}
