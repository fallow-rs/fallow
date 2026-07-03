//! Health result assembly helpers.

use std::time::Duration;

use fallow_config::ResolvedConfig;
use fallow_output::{HealthGrouping, HealthReport, HealthTimings};
use fallow_types::discover::DiscoveredFile;
use fallow_types::workspace::WorkspaceDiagnostic;

use crate::results::HealthAnalysisResult;

use super::HealthExecutionOptions;
use super::css_analytics::{
    HealthScanCtx, StylingAnalysisArtifacts, compute_css_analytics_report_with_artifacts,
};
use super::pipeline::HealthScope;

pub(super) struct HealthOutputParts {
    pub(super) report: HealthReport,
    pub(super) grouping: Option<HealthGrouping>,
    pub(super) timings: Option<HealthTimings>,
    pub(super) coverage_gaps_has_findings: bool,
}

struct HealthReportSideEffectsInput<'a> {
    opts: &'a HealthExecutionOptions<'a>,
    report: &'a mut HealthReport,
    files: &'a [DiscoveredFile],
    /// The per-file extraction output (always present, graph-independent). Used by
    /// the `--css` path to derive the CSS-in-JS design-token blast-radius from
    /// imports + member accesses without a resolved graph (Phase 3d).
    modules: &'a [fallow_types::extract::ModuleInfo],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    dead_code_results: Option<&'a fallow_types::results::AnalysisResults>,
    styling_artifacts: Option<&'a StylingAnalysisArtifacts>,
}

pub(super) struct HealthFinalizeInput<'a, R> {
    pub(super) opts: &'a HealthExecutionOptions<'a>,
    pub(super) config: ResolvedConfig,
    pub(super) files: &'a [DiscoveredFile],
    pub(super) modules: &'a [fallow_types::extract::ModuleInfo],
    pub(super) scope: HealthScope<'a, R>,
    pub(super) output: HealthOutputParts,
    pub(super) elapsed: Duration,
    pub(super) should_fail_on_coverage_gaps: bool,
    pub(super) dead_code_results: Option<&'a fallow_types::results::AnalysisResults>,
    pub(super) styling_artifacts: Option<&'a StylingAnalysisArtifacts>,
    pub(super) workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

struct HealthResultInput<R> {
    config: ResolvedConfig,
    report: HealthReport,
    grouping: Option<HealthGrouping>,
    group_resolver: Option<R>,
    elapsed: Duration,
    timings: Option<HealthTimings>,
    coverage_gaps_has_findings: bool,
    should_fail_on_coverage_gaps: bool,
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

pub(super) fn finalize_health_result<R>(
    input: HealthFinalizeInput<'_, R>,
) -> HealthAnalysisResult<R> {
    let HealthFinalizeInput {
        opts,
        config,
        files,
        modules,
        scope,
        output,
        elapsed,
        should_fail_on_coverage_gaps,
        dead_code_results,
        styling_artifacts,
        workspace_diagnostics,
    } = input;
    let HealthOutputParts {
        mut report,
        grouping,
        timings,
        coverage_gaps_has_findings,
    } = output;

    finalize_health_report_side_effects(&mut HealthReportSideEffectsInput {
        opts,
        report: &mut report,
        files,
        modules,
        config: &config,
        ignore_set: &scope.ignore_set,
        changed_files: scope.changed_files.as_ref(),
        ws_roots: scope.ws_roots.as_deref(),
        dead_code_results,
        styling_artifacts,
    });

    build_health_result(HealthResultInput {
        config,
        report,
        grouping,
        group_resolver: scope.group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    })
}

fn finalize_health_report_side_effects(input: &mut HealthReportSideEffectsInput<'_>) {
    if input.opts.css {
        let scan_changed_files = if input.opts.css_deep {
            None
        } else {
            input.changed_files
        };
        let output_changed_files = input.opts.css_deep.then_some(input.changed_files).flatten();
        let computation = compute_css_analytics_report_with_artifacts(
            input.files,
            input.modules,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: scan_changed_files,
                output_changed_files,
                ws_roots: input.ws_roots,
            },
            input.styling_artifacts,
        );
        input.report.styling_health = computation.as_ref().map(|computation| {
            super::styling_score::compute_styling_health_with_inputs(
                &computation.report,
                &computation.scoring_inputs,
            )
        });
        input.report.css_analytics = computation.map(|computation| computation.report);
        // Graduation (chunk 2): map the descriptive css candidates into first-class
        // styling findings, honoring inline suppression at production time. Styling
        // stays in its own domain (HealthReport), not the dead-code AnalysisResults.
        input.report.styling_findings = build_styling_findings(
            input.report.css_analytics.as_ref(),
            input.modules,
            input.files,
            input.config,
            input.opts.css_deep,
            input.dead_code_results,
        );
    }
}

/// Graduate the descriptive css candidates into first-class `StylingFinding`s.
/// Honors inline suppression (`// fallow-ignore-next-line css-token-drift` /
/// `-file`) at production time, matched by the candidate's relative path against
/// the module's parsed suppressions. First family: `css-token-drift` (Tailwind
/// arbitrary values = hardcoded-instead-of-token). Default severity `warn` is
/// applied downstream; the finding is verdict-neutral by default.
#[expect(
    clippy::too_many_lines,
    reason = "Styling findings are mapped family-by-family so each output shape stays local."
)]
fn build_styling_findings(
    css: Option<&fallow_output::CssAnalyticsReport>,
    modules: &[fallow_types::extract::ModuleInfo],
    files: &[DiscoveredFile],
    config: &ResolvedConfig,
    include_cross_file_reachability: bool,
    dead_code_results: Option<&fallow_types::results::AnalysisResults>,
) -> Vec<fallow_output::StylingFinding> {
    use fallow_config::Severity;
    use fallow_types::suppress::{IssueKind, Suppression, is_file_suppressed, is_suppressed};

    let Some(css) = css else {
        return Vec::new();
    };

    // Shared: relative-path -> the file's parsed suppressions, so every family
    // honors inline suppression at production time.
    let path_by_id: rustc_hash::FxHashMap<_, _> =
        files.iter().map(|f| (f.id, f.path.as_path())).collect();
    let mut supp_by_rel: rustc_hash::FxHashMap<String, &[Suppression]> =
        rustc_hash::FxHashMap::default();
    for module in modules {
        if module.suppressions.is_empty() {
            continue;
        }
        if let Some(abs) = path_by_id.get(&module.file_id)
            && let Some(rel) = super::runtime_filter::relative_to_root(abs, &config.root)
        {
            supp_by_rel.insert(rel, module.suppressions.as_slice());
        }
    }
    let suppressed = |path: &str, line: u32, kind: IssueKind| -> bool {
        supp_by_rel.get(path).is_some_and(|supps| {
            is_file_suppressed(supps, kind) || is_suppressed(supps, line, kind)
        })
    };

    let mut findings = Vec::new();

    // Family css-token-drift: Tailwind arbitrary values (hardcoded-instead-of-token).
    if config.rules.css_token_drift != Severity::Off {
        for candidate in &css.tailwind_arbitrary_values {
            if suppressed(&candidate.path, candidate.line, IssueKind::CssTokenDrift) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-token-drift".to_string(),
                sub_kind: "tailwind-arbitrary-value".to_string(),
                path: candidate.path.clone(),
                line: candidate.line,
                value: candidate.value.clone(),
                effective_severity: styling_finding_severity(config.rules.css_token_drift),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::High),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::FixConfidently),
                nearest_token: None,
                fix_hint: Some(
                    "Replace the one-off Tailwind arbitrary value with an existing scale token, or confirm it is intentional."
                        .to_string(),
                ),
                actions: candidate.actions.clone(),
            });
        }
        for candidate in &css.cva_variant_token_drifts {
            if suppressed(&candidate.path, candidate.line, IssueKind::CssTokenDrift) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-token-drift".to_string(),
                sub_kind: "cva-variant-token-drift".to_string(),
                path: candidate.path.clone(),
                line: candidate.line,
                value: format!(
                    "{} in CVA variant: {}",
                    candidate.class_token, candidate.variant_classes
                ),
                effective_severity: styling_finding_severity(config.rules.css_token_drift),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::Low),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                nearest_token: Some(candidate.nearest_token.clone()),
                fix_hint: Some(format!(
                    "Verify this CVA variant is not an intentional one-off, then reuse {} instead.",
                    candidate.nearest_token.name
                )),
                actions: candidate.actions.clone(),
            });
        }
        for candidate in &css.raw_style_values {
            let Some(nearest_token) = candidate.nearest_token.as_ref() else {
                continue;
            };
            if suppressed(&candidate.path, candidate.line, IssueKind::CssTokenDrift) {
                continue;
            }
            let fix_hint = format!(
                "Verify the raw style value is not an intentional exception, then reuse {} instead.",
                nearest_token.name
            );
            findings.push(fallow_output::StylingFinding {
                code: "css-token-drift".to_string(),
                sub_kind: "raw-style-value".to_string(),
                path: candidate.path.clone(),
                line: candidate.line,
                value: format!(
                    "{} {}: {}",
                    candidate.axis, candidate.property, candidate.value
                ),
                effective_severity: styling_finding_severity(config.rules.css_token_drift),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::Low),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                nearest_token: Some(nearest_token.clone()),
                fix_hint: Some(fix_hint),
                actions: candidate.actions.clone(),
            });
        }
        if include_cross_file_reachability {
            for candidate in &css.near_duplicate_theme_tokens {
                if suppressed(&candidate.path, candidate.line, IssueKind::CssTokenDrift) {
                    continue;
                }
                let semantic_color_alias = near_duplicate_is_semantic_color_alias(candidate);
                let (confidence, agent_disposition) = if semantic_color_alias {
                    (
                        fallow_output::StylingFindingConfidence::Low,
                        fallow_output::StylingAgentDisposition::VerifyFirst,
                    )
                } else {
                    (
                        fallow_output::StylingFindingConfidence::High,
                        fallow_output::StylingAgentDisposition::FixConfidently,
                    )
                };
                findings.push(fallow_output::StylingFinding {
                    code: "css-token-drift".to_string(),
                    sub_kind: "near-duplicate-theme-token".to_string(),
                    path: candidate.path.clone(),
                    line: candidate.line,
                    value: format!("{}: {}", candidate.token, candidate.value),
                    effective_severity: styling_finding_severity(config.rules.css_token_drift),
                    blast_radius: None,
                    confidence: Some(confidence),
                    agent_disposition: Some(agent_disposition),
                    nearest_token: Some(candidate.nearest_token.clone()),
                    fix_hint: Some(format!(
                        "Reuse {} instead of adding {} after verifying the semantic intent.",
                        candidate.nearest_token.name, candidate.token
                    )),
                    actions: candidate.actions.clone(),
                });
            }
        }
    }

    // Family css-duplicate-block: copy-pasted declaration blocks (anchored at the
    // first occurrence).
    if config.rules.css_duplicate_block != Severity::Off {
        for block in &css.duplicate_declaration_blocks {
            let Some(first) = block.occurrences.first() else {
                continue;
            };
            if suppressed(&first.path, first.line, IssueKind::CssDuplicateBlock) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-duplicate-block".to_string(),
                sub_kind: "duplicate-declaration-block".to_string(),
                path: first.path.clone(),
                line: first.line,
                value: format!(
                    "{}-declaration block repeated {} times",
                    block.declaration_count, block.occurrence_count
                ),
                effective_severity: styling_finding_severity(config.rules.css_duplicate_block),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::High),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::FixConfidently),
                nearest_token: None,
                fix_hint: Some(
                    "Consolidate the repeated declaration block after checking cascade order."
                        .to_string(),
                ),
                actions: block.actions.clone(),
            });
        }
        for block in &css.cva_duplicate_variant_blocks {
            let Some(first) = block.occurrences.first() else {
                continue;
            };
            if suppressed(&first.path, first.line, IssueKind::CssDuplicateBlock) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-duplicate-block".to_string(),
                sub_kind: "cva-duplicate-variant-block".to_string(),
                path: first.path.clone(),
                line: first.line,
                value: format!(
                    "CVA variant class block repeated {} times: {}",
                    block.occurrence_count, block.value
                ),
                effective_severity: styling_finding_severity(config.rules.css_duplicate_block),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::High),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::FixConfidently),
                nearest_token: None,
                fix_hint: Some(
                    "Extract the repeated CVA variant classes into a shared base or compound variant after checking variant semantics."
                        .to_string(),
                ),
                actions: block.actions.clone(),
            });
        }
    }

    // Family css-selector-complexity: parser-bounded notable rules (high
    // specificity, deep nesting, or important density), all changed-file-local.
    if config.rules.css_selector_complexity != Severity::Off {
        for file in &css.files {
            for rule in &file.analytics.notable_rules {
                if suppressed(&file.path, rule.line, IssueKind::CssSelectorComplexity) {
                    continue;
                }
                let (sub_kind, value, reason) = selector_complexity_finding(rule);
                let confidence_kind = selector_complexity_confidence_kind(config, &file.path, rule);
                let (confidence, agent_disposition, fix_hint) = if let Some(kind) = confidence_kind
                {
                    (
                        fallow_output::StylingFindingConfidence::Low,
                        fallow_output::StylingAgentDisposition::VerifyFirst,
                        kind.fix_hint(),
                    )
                } else {
                    (
                        fallow_output::StylingFindingConfidence::High,
                        fallow_output::StylingAgentDisposition::FixConfidently,
                        "Simplify the selector or rule after checking cascade impact.",
                    )
                };
                findings.push(fallow_output::StylingFinding {
                    code: "css-selector-complexity".to_string(),
                    sub_kind: sub_kind.to_string(),
                    path: file.path.clone(),
                    line: rule.line,
                    value,
                    effective_severity: styling_finding_severity(
                        config.rules.css_selector_complexity,
                    ),
                    blast_radius: None,
                    confidence: Some(confidence),
                    agent_disposition: Some(agent_disposition),
                    nearest_token: None,
                    fix_hint: Some(fix_hint.to_string()),
                    actions: vec![fallow_output::CssCandidateAction::simplify_selector(reason)],
                });
            }
        }
    }

    // Family css-dead-surface: local scoped SFC classes by default, plus
    // cross-file reachability candidates when deep CSS mode produced them.
    if config.rules.css_dead_surface != Severity::Off {
        append_dead_style_export_findings(&mut findings, dead_code_results, config, &suppressed);
        for candidate in &css.scoped_unused {
            if suppressed(&candidate.path, 1, IssueKind::CssDeadSurface) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-dead-surface".to_string(),
                sub_kind: "scoped-unused-class".to_string(),
                path: candidate.path.clone(),
                line: 1,
                value: format!(
                    "{} scoped {} unused: {}",
                    candidate.classes.len(),
                    if candidate.classes.len() == 1 {
                        "class"
                    } else {
                        "classes"
                    },
                    candidate.classes.join(", ")
                ),
                effective_severity: styling_finding_severity(config.rules.css_dead_surface),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::Low),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                nearest_token: None,
                fix_hint: Some(
                    "Verify no dynamic component-local use exists before removing the scoped class."
                        .to_string(),
                ),
                actions: candidate.actions.clone(),
            });
        }
        if include_cross_file_reachability {
            for candidate in &css.unused_theme_tokens {
                if suppressed(&candidate.path, candidate.line, IssueKind::CssDeadSurface) {
                    continue;
                }
                findings.push(fallow_output::StylingFinding {
                    code: "css-dead-surface".to_string(),
                    sub_kind: "unused-theme-token".to_string(),
                    path: candidate.path.clone(),
                    line: candidate.line,
                    value: candidate.token.clone(),
                    effective_severity: styling_finding_severity(config.rules.css_dead_surface),
                    blast_radius: Some(0),
                    confidence: Some(fallow_output::StylingFindingConfidence::Low),
                    agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                    nearest_token: None,
                    fix_hint: Some(
                        "Verify no external or plugin consumer exists before removing the unused theme token."
                            .to_string(),
                    ),
                    actions: candidate.actions.clone(),
                });
            }
            for candidate in &css.unreferenced_css_classes {
                if suppressed(&candidate.path, candidate.line, IssueKind::CssDeadSurface) {
                    continue;
                }
                findings.push(fallow_output::StylingFinding {
                    code: "css-dead-surface".to_string(),
                    sub_kind: "unreferenced-css-class".to_string(),
                    path: candidate.path.clone(),
                    line: candidate.line,
                    value: candidate.class.clone(),
                    effective_severity: styling_finding_severity(config.rules.css_dead_surface),
                    blast_radius: None,
                    confidence: Some(fallow_output::StylingFindingConfidence::Low),
                    agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                    nearest_token: None,
                    fix_hint: Some(
                        "Verify no dynamic or external markup consumer exists before removing the class."
                            .to_string(),
                    ),
                    actions: candidate.actions.clone(),
                });
            }
            for candidate in &css.unreferenced_keyframes {
                if suppressed(&candidate.path, 1, IssueKind::CssDeadSurface) {
                    continue;
                }
                findings.push(fallow_output::StylingFinding {
                    code: "css-dead-surface".to_string(),
                    sub_kind: "unreferenced-keyframes".to_string(),
                    path: candidate.path.clone(),
                    line: 1,
                    value: candidate.name.clone(),
                    effective_severity: styling_finding_severity(config.rules.css_dead_surface),
                    blast_radius: None,
                    confidence: Some(fallow_output::StylingFindingConfidence::Low),
                    agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                    nearest_token: None,
                    fix_hint: Some(
                        "Verify no JavaScript animation reference exists before removing the keyframes."
                            .to_string(),
                    ),
                    actions: candidate.actions.clone(),
                });
            }
            for candidate in &css.unused_font_faces {
                if suppressed(&candidate.path, 1, IssueKind::CssDeadSurface) {
                    continue;
                }
                findings.push(fallow_output::StylingFinding {
                    code: "css-dead-surface".to_string(),
                    sub_kind: "unused-font-face".to_string(),
                    path: candidate.path.clone(),
                    line: 1,
                    value: candidate.family.clone(),
                    effective_severity: styling_finding_severity(config.rules.css_dead_surface),
                    blast_radius: None,
                    confidence: Some(fallow_output::StylingFindingConfidence::Low),
                    agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                    nearest_token: None,
                    fix_hint: Some(
                        "Verify no inline style or JavaScript font-family use exists before removing the font face."
                            .to_string(),
                    ),
                    actions: candidate.actions.clone(),
                });
            }
            for candidate in &css.unused_at_rules {
                if suppressed(&candidate.path, 1, IssueKind::CssDeadSurface) {
                    continue;
                }
                findings.push(fallow_output::StylingFinding {
                    code: "css-dead-surface".to_string(),
                    sub_kind: match candidate.kind {
                        fallow_output::UnusedAtRuleKind::PropertyRegistration => {
                            "unused-property-registration"
                        }
                        fallow_output::UnusedAtRuleKind::Layer => "unused-layer",
                    }
                    .to_string(),
                    path: candidate.path.clone(),
                    line: 1,
                    value: candidate.name.clone(),
                    effective_severity: styling_finding_severity(config.rules.css_dead_surface),
                    blast_radius: None,
                    confidence: Some(fallow_output::StylingFindingConfidence::Low),
                    agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                    nearest_token: None,
                    fix_hint: Some(
                        "Verify no dynamic stylesheet consumer exists before removing the at-rule."
                            .to_string(),
                    ),
                    actions: candidate.actions.clone(),
                });
            }
        }
    }

    if include_cross_file_reachability && config.rules.css_broken_reference != Severity::Off {
        for candidate in &css.unresolved_class_references {
            if suppressed(
                &candidate.path,
                candidate.line,
                IssueKind::CssBrokenReference,
            ) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-broken-reference".to_string(),
                sub_kind: "unresolved-class-reference".to_string(),
                path: candidate.path.clone(),
                line: candidate.line,
                value: format!("{} -> {}", candidate.class, candidate.suggestion),
                effective_severity: styling_finding_severity(config.rules.css_broken_reference),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::Low),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                nearest_token: None,
                fix_hint: Some(format!(
                    "Verify the class is not defined externally, then replace {} with {}.",
                    candidate.class, candidate.suggestion
                )),
                actions: candidate.actions.clone(),
            });
        }
        for candidate in &css.undefined_keyframes {
            if suppressed(&candidate.path, 1, IssueKind::CssBrokenReference) {
                continue;
            }
            findings.push(fallow_output::StylingFinding {
                code: "css-broken-reference".to_string(),
                sub_kind: "undefined-keyframes".to_string(),
                path: candidate.path.clone(),
                line: 1,
                value: candidate.name.clone(),
                effective_severity: styling_finding_severity(config.rules.css_broken_reference),
                blast_radius: None,
                confidence: Some(fallow_output::StylingFindingConfidence::Low),
                agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
                nearest_token: None,
                fix_hint: Some(
                    "Verify the keyframes are not defined externally before fixing the animation reference."
                        .to_string(),
                ),
                actions: candidate.actions.clone(),
            });
        }
    }

    findings
}

fn append_dead_style_export_findings(
    findings: &mut Vec<fallow_output::StylingFinding>,
    dead_code_results: Option<&fallow_types::results::AnalysisResults>,
    config: &ResolvedConfig,
    suppressed: &impl Fn(&str, u32, fallow_types::suppress::IssueKind) -> bool,
) {
    let Some(results) = dead_code_results else {
        return;
    };
    for finding in &results.unused_exports {
        let export = &finding.export;
        if export.is_type_only || export.is_re_export {
            continue;
        }
        let Some(path) = super::runtime_filter::relative_to_root(&export.path, &config.root) else {
            continue;
        };
        if suppressed(
            &path,
            export.line,
            fallow_types::suppress::IssueKind::CssDeadSurface,
        ) {
            continue;
        }
        let Some(surface) = classify_dead_style_export(export) else {
            continue;
        };
        findings.push(fallow_output::StylingFinding {
            code: "css-dead-surface".to_string(),
            sub_kind: surface.sub_kind.to_string(),
            path,
            line: export.line,
            value: format!("{} ({})", export.export_name, surface.family),
            effective_severity: styling_finding_severity(config.rules.css_dead_surface),
            blast_radius: Some(0),
            confidence: Some(fallow_output::StylingFindingConfidence::Low),
            agent_disposition: Some(fallow_output::StylingAgentDisposition::VerifyFirst),
            nearest_token: None,
            fix_hint: Some(format!(
                "Verify no dynamic styling consumer imports {} before removing the unused {} binding.",
                export.export_name, surface.family
            )),
            actions: vec![fallow_output::CssCandidateAction {
                kind: fallow_output::CssCandidateActionType::VerifyUnused,
                auto_fixable: false,
                description: format!(
                    "Confirm no dynamic import, story, test fixture, or external consumer uses the {} styling binding before removing it.",
                    export.export_name
                ),
                command: None,
            }],
        });
    }
}

struct DeadStyleExportSurface {
    sub_kind: &'static str,
    family: &'static str,
}

fn classify_dead_style_export(
    export: &fallow_types::results::UnusedExport,
) -> Option<DeadStyleExportSurface> {
    let source = std::fs::read_to_string(&export.path).ok()?;
    let window = source_window(&source, export.line, 5);
    if window.contains("styled.") || window.contains("styled(") {
        let family = if source.contains("@emotion/styled") {
            "Emotion"
        } else {
            "styled-components"
        };
        return Some(DeadStyleExportSurface {
            sub_kind: "unused-styled-binding",
            family,
        });
    }
    if window.contains("stylex.create(") || window.contains("stylex.create({") {
        return Some(DeadStyleExportSurface {
            sub_kind: "unused-stylex-binding",
            family: "StyleX",
        });
    }
    if source.contains("@vanilla-extract/css")
        && (window.contains("style(") || window.contains("styleVariants("))
    {
        return Some(DeadStyleExportSurface {
            sub_kind: "unused-vanilla-extract-binding",
            family: "vanilla-extract",
        });
    }
    if source.contains("@emotion/")
        && (window.contains("css`") || window.contains("css(") || window.contains("styled."))
    {
        return Some(DeadStyleExportSurface {
            sub_kind: "unused-emotion-binding",
            family: "Emotion",
        });
    }
    if (source.contains("styled-system") || source.contains("@pandacss"))
        && (window.contains("css(") || window.contains("cva(") || window.contains("recipe("))
    {
        return Some(DeadStyleExportSurface {
            sub_kind: "unused-panda-binding",
            family: "Panda CSS",
        });
    }
    if source.contains("class-variance-authority") && window.contains("cva(") {
        return Some(DeadStyleExportSurface {
            sub_kind: "unused-cva-binding",
            family: "CVA",
        });
    }
    None
}

fn source_window(source: &str, line: u32, lines: usize) -> String {
    let start = line.saturating_sub(1) as usize;
    source
        .lines()
        .skip(start)
        .take(lines)
        .collect::<Vec<_>>()
        .join("\n")
}

fn near_duplicate_is_semantic_color_alias(
    candidate: &fallow_output::NearDuplicateThemeToken,
) -> bool {
    let Some(token_name) = candidate.token.strip_prefix("--color-") else {
        return false;
    };
    let Some(nearest_name) = candidate.nearest_token.name.strip_prefix("--color-") else {
        return false;
    };
    color_token_name_is_semantic_alias(token_name)
        || color_token_name_is_semantic_alias(nearest_name)
}

fn color_token_name_is_semantic_alias(name: &str) -> bool {
    const UI_ROLES: &[&str] = &[
        "accent",
        "accent-foreground",
        "background",
        "border",
        "card",
        "card-foreground",
        "destructive",
        "destructive-foreground",
        "foreground",
        "input",
        "muted",
        "muted-foreground",
        "popover",
        "popover-foreground",
        "primary",
        "primary-foreground",
        "ring",
        "secondary",
        "secondary-foreground",
    ];
    UI_ROLES.contains(&name)
        || name.ends_with("-bg")
        || name.ends_with("-background")
        || name.ends_with("-fg")
        || name.ends_with("-foreground")
        || name.ends_with("-text")
        || name.ends_with("-border")
        || name.ends_with("-surface")
}

fn styling_finding_severity(
    severity: fallow_config::Severity,
) -> fallow_output::StylingFindingSeverity {
    match severity {
        fallow_config::Severity::Error => fallow_output::StylingFindingSeverity::Error,
        fallow_config::Severity::Warn | fallow_config::Severity::Off => {
            fallow_output::StylingFindingSeverity::Warn
        }
    }
}

fn selector_complexity_finding(
    rule: &fallow_types::extract::CssRuleMetric,
) -> (&'static str, String, &'static str) {
    if rule.specificity_a > 0 {
        return (
            "high-specificity",
            format!(
                "specificity {}-{}-{}",
                rule.specificity_a, rule.specificity_b, rule.specificity_c
            ),
            "it uses an id selector",
        );
    }
    if rule.nesting_depth >= 3 {
        return (
            "deep-nesting",
            format!("nesting depth {}", rule.nesting_depth),
            "it is deeply nested",
        );
    }
    if rule.important_count > 0 {
        return (
            "important-density",
            format!(
                "{} !important {} across {} {}",
                rule.important_count,
                if rule.important_count == 1 {
                    "declaration"
                } else {
                    "declarations"
                },
                rule.declaration_count,
                if rule.declaration_count == 1 {
                    "declaration"
                } else {
                    "declarations"
                }
            ),
            "it relies on !important",
        );
    }
    (
        "complex-selector",
        format!("selector complexity {}", rule.complexity),
        "the selector is structurally complex",
    )
}

#[derive(Clone, Copy)]
enum SelectorComplexityConfidenceKind {
    ResetOrAccessibility,
    ThirdPartyGeneratedSurface,
}

impl SelectorComplexityConfidenceKind {
    fn fix_hint(self) -> &'static str {
        match self {
            Self::ResetOrAccessibility => {
                "Verify this reset or accessibility rule is intentional before changing it."
            }
            Self::ThirdPartyGeneratedSurface => {
                "This targets a third-party generated DOM surface, so cleanup is not proven. Verify the override against the library component before changing it."
            }
        }
    }
}

fn selector_complexity_confidence_kind(
    config: &ResolvedConfig,
    path: &str,
    rule: &fallow_types::extract::CssRuleMetric,
) -> Option<SelectorComplexityConfidenceKind> {
    if rule.important_count == 0 {
        return None;
    }
    let full_path = config.root.join(path);
    let Ok(source) = std::fs::read_to_string(full_path) else {
        return None;
    };
    let target_line = usize::try_from(rule.line).unwrap_or(usize::MAX);
    let start_line = target_line.saturating_sub(8).max(1);
    let end_line = target_line.saturating_add(4);
    let mut window = String::new();
    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        if line_no >= start_line && line_no <= end_line {
            window.push_str(line);
            window.push('\n');
        }
    }
    let window = window.to_ascii_lowercase();
    if third_party_override_window(&window) {
        return Some(SelectorComplexityConfidenceKind::ThirdPartyGeneratedSurface);
    }
    if window.contains("prefers-reduced-motion")
        || window.contains("reduced motion")
        || window.contains("pointer: coarse")
        || window.contains("pointer: fine")
        || window.contains(".touch-only")
        || window.contains("accessibility")
    {
        return Some(SelectorComplexityConfidenceKind::ResetOrAccessibility);
    }
    None
}

fn third_party_override_window(window: &str) -> bool {
    [
        "ant-",
        ".ant-",
        "data-sonner",
        "sonner",
        "toastify",
        "data-radix",
        "data-vaul",
        "cmdk-",
        "headlessui",
        "headlessui-",
        "react-select",
        "react-datepicker",
        "maplibre",
        "mapboxgl",
        "mapbox-",
        "recharts",
        "swiper",
        "tippy-",
        "floating-ui",
    ]
    .iter()
    .any(|needle| window.contains(needle))
}

fn build_health_result<R>(input: HealthResultInput<R>) -> HealthAnalysisResult<R> {
    let HealthResultInput {
        config,
        report,
        grouping,
        group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
        workspace_diagnostics,
    } = input;

    HealthAnalysisResult {
        report,
        grouping,
        group_resolver,
        config,
        workspace_diagnostics,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
    }
}
