//! Shared dead-code CodeClimate issue construction.

use std::path::Path;

use fallow_config::{RulesConfig, Severity};
use fallow_output::{
    CodeClimateIssue, CodeClimateIssueInput, CodeClimateSeverity, build_codeclimate_issue,
    codeclimate_fingerprint_hash, normalize_uri,
};
use fallow_types::results::AnalysisResults;

fn severity_to_codeclimate(s: Severity) -> CodeClimateSeverity {
    match s {
        Severity::Error => CodeClimateSeverity::Major,
        Severity::Warn => CodeClimateSeverity::Minor,
        Severity::Off => unreachable!(),
    }
}

fn cc_path(path: &Path, root: &Path) -> String {
    normalize_uri(
        &path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}

fn fingerprint_hash(parts: &[&str]) -> String {
    codeclimate_fingerprint_hash(parts)
}

/// Push CodeClimate issues for unused dependencies with a shared structure.
fn push_dep_cc_issues<'a, I>(
    issues: &mut Vec<CodeClimateIssue>,
    deps: I,
    root: &Path,
    rule_id: &str,
    location_label: &str,
    severity: Severity,
) where
    I: IntoIterator<Item = &'a fallow_types::results::UnusedDependency>,
{
    for dep in deps {
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&[rule_id, &dep.package_name]);
        let workspace_context = if dep.used_in_workspaces.is_empty() {
            String::new()
        } else {
            let workspaces = dep
                .used_in_workspaces
                .iter()
                .map(|path| cc_path(path, root))
                .collect::<Vec<_>>()
                .join(", ");
            format!("; imported in other workspaces: {workspaces}")
        };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: rule_id,
            description: &format!(
                "Package '{}' is in {location_label} but never imported{workspace_context}",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_file_issues(
    issues: &mut Vec<CodeClimateIssue>,
    files: &[fallow_types::output_dead_code::UnusedFileFinding],
    root: &Path,
    severity: Severity,
) {
    if files.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in files {
        let path = cc_path(&entry.file.path, root);
        let fp = fingerprint_hash(&["fallow/unused-file", &path]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-file",
            description: "File is not reachable from any entry point",
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: None,
            fingerprint: &fp,
        }));
    }
}

/// Push CodeClimate issues for unused exports or unused types.
///
/// `direct_label` / `re_export_label` let the same helper produce the right
/// prose for both `unused-export` (Export / Re-export) and `unused-type`
/// (Type export / Type re-export) rule ids.
struct UnusedExportIssuesInput<'a, I> {
    issues: &'a mut Vec<CodeClimateIssue>,
    exports: I,
    root: &'a Path,
    rule_id: &'a str,
    direct_label: &'a str,
    re_export_label: &'a str,
    severity: Severity,
}

fn push_unused_export_issues<'a, I>(input: UnusedExportIssuesInput<'a, I>)
where
    I: IntoIterator<Item = &'a fallow_types::results::UnusedExport>,
{
    for export in input.exports {
        let level = severity_to_codeclimate(input.severity);
        let path = cc_path(&export.path, input.root);
        let kind = if export.is_re_export {
            input.re_export_label
        } else {
            input.direct_label
        };
        let line_str = export.line.to_string();
        let fp = fingerprint_hash(&[input.rule_id, &path, &line_str, &export.export_name]);
        input
            .issues
            .push(build_codeclimate_issue(CodeClimateIssueInput {
                check_name: input.rule_id,
                description: &format!(
                    "{kind} '{}' is never imported by other modules",
                    export.export_name
                ),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(export.line),
                fingerprint: &fp,
            }));
    }
}

fn push_private_type_leak_issues(
    issues: &mut Vec<CodeClimateIssue>,
    leaks: &[fallow_types::output_dead_code::PrivateTypeLeakFinding],
    root: &Path,
    severity: Severity,
) {
    if leaks.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in leaks {
        let leak = &entry.leak;
        let path = cc_path(&leak.path, root);
        let line_str = leak.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/private-type-leak",
            &path,
            &line_str,
            &leak.export_name,
            &leak.type_name,
        ]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/private-type-leak",
            description: &format!(
                "Export '{}' references private type '{}'",
                leak.export_name, leak.type_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(leak.line),
            fingerprint: &fp,
        }));
    }
}

fn push_type_only_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_types::output_dead_code::TypeOnlyDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/type-only-dependency", &dep.package_name]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/type-only-dependency",
            description: &format!(
                "Package '{}' is only imported via type-only imports (consider moving to devDependencies)",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_test_only_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_types::output_dead_code::TestOnlyDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/test-only-dependency", &dep.package_name]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/test-only-dependency",
            description: &format!(
                "Package '{}' is only imported by test files (consider moving to devDependencies)",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_dev_dep_in_prod_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_types::output_dead_code::DevDependencyInProductionFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/dev-dependency-in-production", &dep.package_name]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/dev-dependency-in-production",
            description: &format!(
                "devDependency '{}' is imported by production code at runtime (consider moving to dependencies)",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

/// Push CodeClimate issues for unused enum or class members.
///
/// `entity_label` is `"Enum"` or `"Class"` so the rendered description reads
/// "Enum member ..." or "Class member ..." accordingly.
fn push_unused_member_issues<'a, I>(
    issues: &mut Vec<CodeClimateIssue>,
    members: I,
    root: &Path,
    rule_id: &str,
    entity_label: &str,
    severity: Severity,
) where
    I: IntoIterator<Item = &'a fallow_types::results::UnusedMember>,
{
    for member in members {
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&member.path, root);
        let line_str = member.line.to_string();
        let fp = fingerprint_hash(&[
            rule_id,
            &path,
            &line_str,
            &member.parent_name,
            &member.member_name,
        ]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: rule_id,
            description: &format!(
                "{entity_label} member '{}.{}' is never referenced",
                member.parent_name, member.member_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(member.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unresolved_import_issues(
    issues: &mut Vec<CodeClimateIssue>,
    imports: &[fallow_types::output_dead_code::UnresolvedImportFinding],
    root: &Path,
    severity: Severity,
) {
    if imports.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in imports {
        let import = &entry.import;
        let path = cc_path(&import.path, root);
        let line_str = import.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unresolved-import",
            &path,
            &line_str,
            &import.specifier,
        ]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unresolved-import",
            description: &format!("Import '{}' could not be resolved", import.specifier),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(import.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unlisted_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_types::output_dead_code::UnlistedDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        for site in &dep.imported_from {
            let path = cc_path(&site.path, root);
            let line_str = site.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/unlisted-dependency",
                &path,
                &line_str,
                &dep.package_name,
            ]);
            issues.push(build_codeclimate_issue(CodeClimateIssueInput {
                check_name: "fallow/unlisted-dependency",
                description: &format!(
                    "Package '{}' is imported but not listed in package.json",
                    dep.package_name
                ),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(site.line),
                fingerprint: &fp,
            }));
        }
    }
}

fn push_duplicate_export_issues(
    issues: &mut Vec<CodeClimateIssue>,
    dups: &[fallow_types::output_dead_code::DuplicateExportFinding],
    root: &Path,
    severity: Severity,
) {
    if dups.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dup in dups {
        let dup = &dup.export;
        for loc in &dup.locations {
            let path = cc_path(&loc.path, root);
            let line_str = loc.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/duplicate-export",
                &path,
                &line_str,
                &dup.export_name,
            ]);
            issues.push(build_codeclimate_issue(CodeClimateIssueInput {
                check_name: "fallow/duplicate-export",
                description: &format!("Export '{}' appears in multiple modules", dup.export_name),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(loc.line),
                fingerprint: &fp,
            }));
        }
    }
}

fn push_circular_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::CircularDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in cycles {
        let cycle = &entry.cycle;
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let fp = fingerprint_hash(&["fallow/circular-dependency", &chain_str]);
        let line = if cycle.line > 0 {
            Some(cycle.line)
        } else {
            None
        };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/circular-dependency",
            description: &format!(
                "Circular dependency{}: {}",
                if cycle.is_cross_package {
                    " (cross-package)"
                } else {
                    ""
                },
                chain.join(" \u{2192} ")
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_re_export_cycle_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::ReExportCycleFinding],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in cycles {
        let cycle = &entry.cycle;
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let kind_token = match cycle.kind {
            fallow_types::results::ReExportCycleKind::SelfLoop => "self-loop",
            fallow_types::results::ReExportCycleKind::MultiNode => "multi-node",
        };
        let kind_tag = match cycle.kind {
            fallow_types::results::ReExportCycleKind::SelfLoop => " (self-loop)",
            fallow_types::results::ReExportCycleKind::MultiNode => "",
        };
        let fp = fingerprint_hash(&["fallow/re-export-cycle", kind_token, &chain_str]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/re-export-cycle",
            description: &format!("Re-export cycle{}: {}", kind_tag, chain.join(" <-> ")),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: None,
            fingerprint: &fp,
        }));
    }
}

fn push_boundary_violation_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.from_path, root);
        let to = cc_path(&v.to_path, root);
        let fp = fingerprint_hash(&["fallow/boundary-violation", &path, &to]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/boundary-violation",
            description: &format!(
                "Boundary violation: {} -> {} ({} -> {})",
                path, to, v.from_zone, v.to_zone
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_boundary_coverage_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryCoverageViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.path, root);
        let fp = fingerprint_hash(&["fallow/boundary-coverage", &path]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/boundary-coverage",
            description: &format!("Boundary coverage: {path} matches no configured zone"),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_boundary_call_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryCallViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.path, root);
        let fp = fingerprint_hash(&["fallow/boundary-call-violation", &path, &v.callee]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/boundary-call-violation",
            description: &format!(
                "Boundary call: `{}` matches forbidden pattern `{}` in zone '{}'",
                v.callee, v.pattern, v.zone
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_policy_violation_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::PolicyViolationFinding],
    root: &Path,
) {
    use fallow_types::results::PolicyViolationSeverity;

    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.path, root);
        let rule = format!("{}/{}", v.pack, v.rule_id);
        let fp = fingerprint_hash(&["fallow/policy-violation", &path, &rule, &v.matched]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        // Severity comes from the EFFECTIVE per-finding value, not the
        // policy-violation master, so a severity: "error" rule under a warn
        // master maps to blocker-level just like the exit-code gate.
        let level = severity_to_codeclimate(match v.severity {
            PolicyViolationSeverity::Error => Severity::Error,
            PolicyViolationSeverity::Warn => Severity::Warn,
        });
        let message = match &v.message {
            Some(message) => format!(
                "Policy violation: `{}` is banned by `{rule}`. {message}",
                v.matched
            ),
            None => format!("Policy violation: `{}` is banned by `{rule}`", v.matched),
        };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/policy-violation",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_invalid_client_export_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::InvalidClientExportFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let e = &entry.export;
        let path = cc_path(&e.path, root);
        let fp = fingerprint_hash(&["fallow/invalid-client-export", &path, &e.export_name]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "Export `{}` is not allowed in a \"{}\" file (Next.js server-only / route-config name)",
            e.export_name, e.directive
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/invalid-client-export",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_mixed_client_server_barrel_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::MixedClientServerBarrelFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let b = &entry.barrel;
        let path = cc_path(&b.path, root);
        let fp = fingerprint_hash(&[
            "fallow/mixed-client-server-barrel",
            &path,
            &b.client_origin,
            &b.server_origin,
        ]);
        let line = if b.line > 0 { Some(b.line) } else { None };
        let message = format!(
            "Barrel re-exports both a \"use client\" module (`{}`) and a server-only module (`{}`); one import drags the other's directive across the boundary",
            b.client_origin, b.server_origin
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/mixed-client-server-barrel",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_misplaced_directive_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::MisplacedDirectiveFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let d = &entry.directive_site;
        let path = cc_path(&d.path, root);
        let fp = fingerprint_hash(&[
            "fallow/misplaced-directive",
            &path,
            &d.line.to_string(),
            &d.directive,
        ]);
        let line = if d.line > 0 { Some(d.line) } else { None };
        let message = format!(
            "Directive `\"{}\"` is not in the leading position, so the RSC bundler ignores it; move it to the top of the file",
            d.directive
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/misplaced-directive",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unprovided_inject_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnprovidedInjectFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let i = &entry.inject;
        let path = cc_path(&i.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unprovided-inject",
            &path,
            &i.line.to_string(),
            &i.key_name,
        ]);
        let line = if i.line > 0 { Some(i.line) } else { None };
        let message = format!(
            "inject(`{}`) has no matching provide(`{}`) in this project; at runtime it returns undefined (provide the key or remove this inject)",
            i.key_name, i.key_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unprovided-inject",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unrendered_component_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnrenderedComponentFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let c = &entry.component;
        let path = cc_path(&c.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unrendered-component",
            &path,
            &c.line.to_string(),
            &c.component_name,
        ]);
        let line = if c.line > 0 { Some(c.line) } else { None };
        let message = format!(
            "component `{}` is reachable but rendered nowhere in this project (render it somewhere or remove it)",
            c.component_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unrendered-component",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_prop_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentPropFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let p = &entry.prop;
        let path = cc_path(&p.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-prop",
            &path,
            &p.line.to_string(),
            &p.prop_name,
        ]);
        let line = if p.line > 0 { Some(p.line) } else { None };
        let message = format!(
            "prop `{}` is declared but referenced nowhere in component `{}` (remove it or use it)",
            p.prop_name, p.component_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-component-prop",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_emit_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentEmitFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let e = &entry.emit;
        let path = cc_path(&e.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-emit",
            &path,
            &e.line.to_string(),
            &e.emit_name,
        ]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "emit `{}` is declared but emitted nowhere in component `{}` (remove it or emit it)",
            e.emit_name, e.component_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-component-emit",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_svelte_event_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedSvelteEventFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let e = &entry.event;
        let path = cc_path(&e.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-svelte-event",
            &path,
            &e.line.to_string(),
            &e.event_name,
        ]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "event `{}` is dispatched by component `{}` but listened to nowhere in the project (remove it or listen for it)",
            e.event_name, e.component_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-svelte-event",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_input_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentInputFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let i = &entry.input;
        let path = cc_path(&i.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-input",
            &path,
            &i.line.to_string(),
            &i.input_name,
        ]);
        let line = if i.line > 0 { Some(i.line) } else { None };
        let message = format!(
            "input `{}` is declared but referenced nowhere in component `{}` (remove it or use it)",
            i.input_name, i.component_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-component-input",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_output_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentOutputFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let o = &entry.output;
        let path = cc_path(&o.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-output",
            &path,
            &o.line.to_string(),
            &o.output_name,
        ]);
        let line = if o.line > 0 { Some(o.line) } else { None };
        let message = format!(
            "output `{}` is declared but emitted nowhere in component `{}` (remove it or emit it)",
            o.output_name, o.component_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-component-output",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_server_action_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedServerActionFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let a = &entry.action;
        let path = cc_path(&a.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-server-action",
            &path,
            &a.line.to_string(),
            &a.action_name,
        ]);
        let line = if a.line > 0 { Some(a.line) } else { None };
        let message = format!(
            "server action `{}` is exported from a \"use server\" file but no code in this project references it (wire it to a consumer or remove it)",
            a.action_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-server-action",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_load_data_key_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedLoadDataKeyFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let k = &entry.key;
        let path = cc_path(&k.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-load-data-key",
            &path,
            &k.line.to_string(),
            &k.key_name,
        ]);
        let line = if k.line > 0 { Some(k.line) } else { None };
        let message = format!(
            "load() return key `{}` is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>); delete the key or wire a consumer",
            k.key_name
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-load-data-key",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_route_collision_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::RouteCollisionFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let c = &entry.collision;
        let path = cc_path(&c.path, root);
        let fp = fingerprint_hash(&["fallow/route-collision", &path, &c.url]);
        let line = if c.line > 0 { Some(c.line) } else { None };
        let message = format!(
            "Route file resolves to `{}`, also owned by {} other file(s); Next.js fails the build because a URL can have only one owner",
            c.url,
            c.conflicting_paths.len()
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/route-collision",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_dynamic_segment_name_conflict_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::DynamicSegmentNameConflictFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let c = &entry.conflict;
        let path = cc_path(&c.path, root);
        let fp = fingerprint_hash(&["fallow/dynamic-segment-name-conflict", &path, &c.position]);
        let line = if c.line > 0 { Some(c.line) } else { None };
        let message = format!(
            "Dynamic segments at `{}` use different slug names ({}); Next.js requires one consistent name per dynamic path",
            c.position,
            c.conflicting_segments.join(", ")
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/dynamic-segment-name-conflict",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_stale_suppression_issues(
    issues: &mut Vec<CodeClimateIssue>,
    suppressions: &[fallow_types::results::StaleSuppression],
    root: &Path,
    rules: &RulesConfig,
) {
    if suppressions.is_empty() {
        return;
    }
    for s in suppressions {
        let severity = if s.missing_reason {
            rules.require_suppression_reason
        } else {
            rules.stale_suppressions
        };
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&s.path, root);
        let line_str = s.line.to_string();
        let check_name = if s.missing_reason {
            "fallow/missing-suppression-reason"
        } else {
            "fallow/stale-suppression"
        };
        let fp = fingerprint_hash(&[check_name, &path, &line_str]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name,
            description: &s.display_message(),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(s.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unused_catalog_entry_issues(
    issues: &mut Vec<CodeClimateIssue>,
    entries: &[fallow_types::output_dead_code::UnusedCatalogEntryFinding],
    root: &Path,
    severity: Severity,
) {
    if entries.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in entries {
        let entry = &entry.entry;
        let path = cc_path(&entry.path, root);
        let line_str = entry.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unused-catalog-entry",
            &path,
            &line_str,
            &entry.catalog_name,
            &entry.entry_name,
        ]);
        let description = if entry.catalog_name == "default" {
            format!(
                "Catalog entry '{}' is not referenced by any workspace package",
                entry.entry_name
            )
        } else {
            format!(
                "Catalog entry '{}' (catalog '{}') is not referenced by any workspace package",
                entry.entry_name, entry.catalog_name
            )
        };
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-catalog-entry",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(entry.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unresolved_catalog_reference_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let finding = &finding.reference;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unresolved-catalog-reference",
            &path,
            &line_str,
            &finding.catalog_name,
            &finding.entry_name,
        ]);
        let catalog_phrase = if finding.catalog_name == "default" {
            "the default catalog".to_string()
        } else {
            format!("catalog '{}'", finding.catalog_name)
        };
        let mut description = format!(
            "Package '{}' is referenced via `catalog:{}` but {} does not declare it; `pnpm install` will fail",
            finding.entry_name,
            if finding.catalog_name == "default" {
                ""
            } else {
                finding.catalog_name.as_str()
            },
            catalog_phrase,
        );
        if !finding.available_in_catalogs.is_empty() {
            use std::fmt::Write as _;
            let _ = write!(
                description,
                " (available in: {})",
                finding.available_in_catalogs.join(", ")
            );
        }
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unresolved-catalog-reference",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }));
    }
}

fn push_empty_catalog_group_issues(
    issues: &mut Vec<CodeClimateIssue>,
    groups: &[fallow_types::output_dead_code::EmptyCatalogGroupFinding],
    root: &Path,
    severity: Severity,
) {
    if groups.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for group in groups {
        let group = &group.group;
        let path = cc_path(&group.path, root);
        let line_str = group.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/empty-catalog-group",
            &path,
            &line_str,
            &group.catalog_name,
        ]);
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/empty-catalog-group",
            description: &format!("Catalog group '{}' has no entries", group.catalog_name),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(group.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unused_dependency_override_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedDependencyOverrideFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let finding = &finding.entry;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unused-dependency-override",
            &path,
            &line_str,
            finding.source.as_label(),
            &finding.raw_key,
        ]);
        let mut description = format!(
            "Override `{}` forces version `{}` but `{}` is not declared by any workspace package or resolved in pnpm-lock.yaml",
            finding.raw_key, finding.version_range, finding.target_package,
        );
        if let Some(hint) = &finding.hint {
            use std::fmt::Write as _;
            let _ = write!(description, " ({hint})");
        }
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/unused-dependency-override",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }));
    }
}

fn push_misconfigured_dependency_override_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::MisconfiguredDependencyOverrideFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let finding = &finding.entry;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/misconfigured-dependency-override",
            &path,
            &line_str,
            finding.source.as_label(),
            &finding.raw_key,
        ]);
        let description = format!(
            "Override `{}` -> `{}` is malformed: {}",
            finding.raw_key,
            finding.raw_value,
            finding.reason.describe(),
        );
        issues.push(build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/misconfigured-dependency-override",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }));
    }
}

/// Build CodeClimate issues from dead-code analysis results.
///
/// Returns the typed [`CodeClimateIssue`] vec; callers that emit the wire
/// shape convert via [`fallow_output::codeclimate_issues_to_value`]. The schema
/// drift gate locks the per-issue shape against
/// [`fallow_output::CodeClimateOutput`].
#[must_use]
pub fn build_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> Vec<CodeClimateIssue> {
    CodeClimateBuilder {
        issues: Vec::new(),
        results,
        root,
        rules,
    }
    .build()
}

struct CodeClimateBuilder<'a> {
    issues: Vec<CodeClimateIssue>,
    results: &'a AnalysisResults,
    root: &'a Path,
    rules: &'a RulesConfig,
}

impl CodeClimateBuilder<'_> {
    fn build(mut self) -> Vec<CodeClimateIssue> {
        self.push_file_and_export_issues();
        self.push_private_type_leak_issues();
        self.push_package_dependency_issues();
        self.push_type_test_dependency_issues();
        self.push_member_issues();
        self.push_import_and_duplicate_issues();
        self.push_graph_issues();
        self.push_boundary_issues();
        self.push_suppression_and_catalog_issues();
        self.push_override_issues();
        self.issues
    }

    fn push_file_and_export_issues(&mut self) {
        push_unused_file_issues(
            &mut self.issues,
            &self.results.unused_files,
            self.root,
            self.rules.unused_files,
        );
        push_unused_export_issues(UnusedExportIssuesInput {
            issues: &mut self.issues,
            exports: self.results.unused_exports.iter().map(|e| &e.export),
            root: self.root,
            rule_id: "fallow/unused-export",
            direct_label: "Export",
            re_export_label: "Re-export",
            severity: self.rules.unused_exports,
        });
        push_unused_export_issues(UnusedExportIssuesInput {
            issues: &mut self.issues,
            exports: self.results.unused_types.iter().map(|e| &e.export),
            root: self.root,
            rule_id: "fallow/unused-type",
            direct_label: "Type export",
            re_export_label: "Type re-export",
            severity: self.rules.unused_types,
        });
    }

    fn push_private_type_leak_issues(&mut self) {
        push_private_type_leak_issues(
            &mut self.issues,
            &self.results.private_type_leaks,
            self.root,
            self.rules.private_type_leaks,
        );
    }

    fn push_package_dependency_issues(&mut self) {
        push_dep_cc_issues(
            &mut self.issues,
            self.results.unused_dependencies.iter().map(|f| &f.dep),
            self.root,
            "fallow/unused-dependency",
            "dependencies",
            self.rules.unused_dependencies,
        );
        push_dep_cc_issues(
            &mut self.issues,
            self.results.unused_dev_dependencies.iter().map(|f| &f.dep),
            self.root,
            "fallow/unused-dev-dependency",
            "devDependencies",
            self.rules.unused_dev_dependencies,
        );
        push_dep_cc_issues(
            &mut self.issues,
            self.results
                .unused_optional_dependencies
                .iter()
                .map(|f| &f.dep),
            self.root,
            "fallow/unused-optional-dependency",
            "optionalDependencies",
            self.rules.unused_optional_dependencies,
        );
    }

    fn push_type_test_dependency_issues(&mut self) {
        push_type_only_dep_issues(
            &mut self.issues,
            &self.results.type_only_dependencies,
            self.root,
            self.rules.type_only_dependencies,
        );
        push_test_only_dep_issues(
            &mut self.issues,
            &self.results.test_only_dependencies,
            self.root,
            self.rules.test_only_dependencies,
        );
        push_dev_dep_in_prod_issues(
            &mut self.issues,
            &self.results.dev_dependencies_in_production,
            self.root,
            self.rules.dev_dependencies_in_production,
        );
    }

    fn push_member_issues(&mut self) {
        push_unused_member_issues(
            &mut self.issues,
            self.results.unused_enum_members.iter().map(|m| &m.member),
            self.root,
            "fallow/unused-enum-member",
            "Enum",
            self.rules.unused_enum_members,
        );
        push_unused_member_issues(
            &mut self.issues,
            self.results.unused_class_members.iter().map(|m| &m.member),
            self.root,
            "fallow/unused-class-member",
            "Class",
            self.rules.unused_class_members,
        );
        push_unused_member_issues(
            &mut self.issues,
            self.results.unused_store_members.iter().map(|m| &m.member),
            self.root,
            "fallow/unused-store-member",
            "Store",
            self.rules.unused_store_members,
        );
    }

    fn push_import_and_duplicate_issues(&mut self) {
        push_unresolved_import_issues(
            &mut self.issues,
            &self.results.unresolved_imports,
            self.root,
            self.rules.unresolved_imports,
        );
        push_unlisted_dep_issues(
            &mut self.issues,
            &self.results.unlisted_dependencies,
            self.root,
            self.rules.unlisted_dependencies,
        );
        push_duplicate_export_issues(
            &mut self.issues,
            &self.results.duplicate_exports,
            self.root,
            self.rules.duplicate_exports,
        );
    }

    fn push_graph_issues(&mut self) {
        push_circular_dep_issues(
            &mut self.issues,
            &self.results.circular_dependencies,
            self.root,
            self.rules.circular_dependencies,
        );
        push_re_export_cycle_issues(
            &mut self.issues,
            &self.results.re_export_cycles,
            self.root,
            self.rules.re_export_cycle,
        );
    }

    fn push_boundary_issues(&mut self) {
        self.push_architecture_boundary_issues();
        self.push_client_server_boundary_issues();
        self.push_component_boundary_issues();
        self.push_framework_route_issues();
    }

    fn push_architecture_boundary_issues(&mut self) {
        push_boundary_violation_issues(
            &mut self.issues,
            &self.results.boundary_violations,
            self.root,
            self.rules.boundary_violation,
        );
        push_boundary_coverage_issues(
            &mut self.issues,
            &self.results.boundary_coverage_violations,
            self.root,
            self.rules.boundary_violation,
        );
        push_boundary_call_issues(
            &mut self.issues,
            &self.results.boundary_call_violations,
            self.root,
            self.rules.boundary_violation,
        );
        push_policy_violation_issues(&mut self.issues, &self.results.policy_violations, self.root);
    }

    fn push_client_server_boundary_issues(&mut self) {
        push_invalid_client_export_issues(
            &mut self.issues,
            &self.results.invalid_client_exports,
            self.root,
            self.rules.invalid_client_export,
        );
        push_mixed_client_server_barrel_issues(
            &mut self.issues,
            &self.results.mixed_client_server_barrels,
            self.root,
            self.rules.mixed_client_server_barrel,
        );
        push_misplaced_directive_issues(
            &mut self.issues,
            &self.results.misplaced_directives,
            self.root,
            self.rules.misplaced_directive,
        );
    }

    fn push_component_boundary_issues(&mut self) {
        push_unprovided_inject_issues(
            &mut self.issues,
            &self.results.unprovided_injects,
            self.root,
            self.rules.unprovided_injects,
        );
        push_unrendered_component_issues(
            &mut self.issues,
            &self.results.unrendered_components,
            self.root,
            self.rules.unrendered_components,
        );
        push_unused_component_prop_issues(
            &mut self.issues,
            &self.results.unused_component_props,
            self.root,
            self.rules.unused_component_props,
        );
        push_unused_component_emit_issues(
            &mut self.issues,
            &self.results.unused_component_emits,
            self.root,
            self.rules.unused_component_emits,
        );
        push_unused_component_input_issues(
            &mut self.issues,
            &self.results.unused_component_inputs,
            self.root,
            self.rules.unused_component_inputs,
        );
        push_unused_component_output_issues(
            &mut self.issues,
            &self.results.unused_component_outputs,
            self.root,
            self.rules.unused_component_outputs,
        );
        push_unused_svelte_event_issues(
            &mut self.issues,
            &self.results.unused_svelte_events,
            self.root,
            self.rules.unused_svelte_events,
        );
    }

    fn push_framework_route_issues(&mut self) {
        push_unused_server_action_issues(
            &mut self.issues,
            &self.results.unused_server_actions,
            self.root,
            self.rules.unused_server_actions,
        );
        push_unused_load_data_key_issues(
            &mut self.issues,
            &self.results.unused_load_data_keys,
            self.root,
            self.rules.unused_load_data_keys,
        );
        push_route_collision_issues(
            &mut self.issues,
            &self.results.route_collisions,
            self.root,
            self.rules.route_collision,
        );
        push_dynamic_segment_name_conflict_issues(
            &mut self.issues,
            &self.results.dynamic_segment_name_conflicts,
            self.root,
            self.rules.dynamic_segment_name_conflict,
        );
    }

    fn push_suppression_and_catalog_issues(&mut self) {
        push_stale_suppression_issues(
            &mut self.issues,
            &self.results.stale_suppressions,
            self.root,
            self.rules,
        );
        push_unused_catalog_entry_issues(
            &mut self.issues,
            &self.results.unused_catalog_entries,
            self.root,
            self.rules.unused_catalog_entries,
        );
        push_empty_catalog_group_issues(
            &mut self.issues,
            &self.results.empty_catalog_groups,
            self.root,
            self.rules.empty_catalog_groups,
        );
        push_unresolved_catalog_reference_issues(
            &mut self.issues,
            &self.results.unresolved_catalog_references,
            self.root,
            self.rules.unresolved_catalog_references,
        );
    }

    fn push_override_issues(&mut self) {
        push_unused_dependency_override_issues(
            &mut self.issues,
            &self.results.unused_dependency_overrides,
            self.root,
            self.rules.unused_dependency_overrides,
        );
        push_misconfigured_dependency_override_issues(
            &mut self.issues,
            &self.results.misconfigured_dependency_overrides,
            self.root,
            self.rules.misconfigured_dependency_overrides,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fallow_output::issue_output_contracts;

    fn codeclimate_check_name_literals() -> BTreeSet<String> {
        let source = include_str!("dead_code_codeclimate.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("source before tests");
        let mut literals = BTreeSet::new();
        let mut rest = source;
        while let Some(start) = rest.find("\"fallow/") {
            let after_quote = &rest[start + 1..];
            let Some(end) = after_quote.find('"') else {
                break;
            };
            literals.insert(after_quote[..end].to_owned());
            rest = &after_quote[end + 1..];
        }
        literals
    }

    #[test]
    fn codeclimate_check_names_match_issue_contracts() {
        let from_emitter = codeclimate_check_name_literals();
        let from_contracts = issue_output_contracts()
            .flat_map(|contract| contract.codeclimate_check_names)
            .collect::<BTreeSet<_>>();

        assert_eq!(from_emitter, from_contracts);
    }
}
