//! Shared dead-code SARIF output assembly.

use std::path::Path;

use crate::{
    SarifDocumentInput, SarifFindingFields as SarifFields,
    SarifSourceSnippetCache as SourceSnippetCache, append_sarif_findings as push_sarif_results,
    build_sarif_document, build_sarif_result_with_snippet as sarif_result_with_snippet,
    issue_output_contracts, normalize_uri,
};
use fallow_config::{RulesConfig, Severity};
use fallow_types::{
    issue_meta::issue_sarif_rule_description,
    output_dead_code::*,
    results::{
        AnalysisResults, BoundaryCallViolation, BoundaryCoverageViolation, BoundaryViolation,
        CircularDependency, DeprecatedExportInUse, DevDependencyInProduction, DuplicatePropShape,
        DynamicSegmentNameConflict, InvalidClientExport, MisplacedDirective,
        MixedClientServerBarrel, PolicyViolation, PolicyViolationSeverity, PrivateTypeLeak,
        PropDrillingChain, RouteCollision, StaleSuppression, TestOnlyDependency, ThinWrapper,
        TypeOnlyDependency, UnprovidedInject, UnrenderedComponent, UnresolvedImport,
        UnusedComponentEmit, UnusedComponentInput, UnusedComponentOutput, UnusedComponentProp,
        UnusedDependency, UnusedExport, UnusedFile, UnusedMember, UnusedServerAction,
        UnusedSvelteEvent,
    },
};

fn relative_uri(path: &Path, root: &Path) -> String {
    normalize_uri(
        &path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}

/// Read-only context threaded through the SARIF result builders: the
/// analysis results, project root, and rule severities. Bundled so the
/// `push_*_sarif_results` family shares one parameter instead of three.
#[derive(Clone, Copy)]
struct SarifCtx<'a> {
    results: &'a AnalysisResults,
    root: &'a Path,
    rules: &'a RulesConfig,
}

fn severity_to_sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warn => "warning",
        Severity::Off => unreachable!(),
    }
}

/// The CI severity of a type that never gates the exit code.
///
/// Prop-drilling, thin-wrapper and duplicate-prop-shape findings are health
/// signals: they never fail the run. An `error` rule is capped at `warn`, so
/// a CI system that reads the level never shows an error for a run that
/// passed. `off` stays `off`.
const fn non_gating_severity(rule: Severity) -> Severity {
    match rule {
        Severity::Error => Severity::Warn,
        other => other,
    }
}

/// The SARIF level for one finding: its gate severity when the finding
/// carries one, otherwise the configured rule severity (a saved report from an
/// older version has no gate severity).
fn finding_level(finding: &impl GatedFinding, rule: Severity) -> &'static str {
    match finding.effective_severity() {
        Some(EffectiveSeverity::Error) => "error",
        Some(EffectiveSeverity::Warn) => "warning",
        None => severity_to_sarif_level(rule),
    }
}

fn configured_sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::Error | Severity::Warn => severity_to_sarif_level(s),
        Severity::Off => "none",
    }
}

/// Extract SARIF fields for an unused export or type export.
fn sarif_export_fields(
    export: &UnusedExport,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    kind: &str,
    re_kind: &str,
) -> SarifFields {
    let label = if export.is_re_export { re_kind } else { kind };
    SarifFields {
        rule_id,
        level,
        message: format!(
            "{} '{}' is never imported by other modules",
            label, export.export_name
        ),
        uri: relative_uri(&export.path, root),
        region: Some((export.line, export.col + 1)),
        source_path: Some(export.path.clone()),
        properties: if export.is_re_export {
            Some(serde_json::json!({ "is_re_export": true }))
        } else {
            None
        },
    }
}

fn sarif_private_type_leak_fields(
    leak: &PrivateTypeLeak,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/private-type-leak",
        level,
        message: format!(
            "Export '{}' references private type '{}'",
            leak.export_name, leak.type_name
        ),
        uri: relative_uri(&leak.path, root),
        region: Some((leak.line, leak.col + 1)),
        source_path: Some(leak.path.clone()),
        properties: None,
    }
}

fn sarif_deprecated_export_fields(
    export: &DeprecatedExportInUse,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/deprecated-export-in-use",
        level,
        message: export.description(),
        uri: relative_uri(&export.path, root),
        region: Some((export.line, export.col + 1)),
        source_path: Some(export.path.clone()),
        properties: Some(serde_json::json!({
            "consumer_count": export.consumer_count,
            "public_api": export.public_api,
        })),
    }
}

/// 1-based column of the `"<name>"` key inside the manifest line a dependency
/// finding points at, falling back to 1 when the line cannot be read.
///
/// Every dependency declared on one line otherwise reports the same location,
/// and a SARIF fingerprint is rule id plus location plus source snippet: a
/// compact `package.json` collapsed all of its unused dependencies into one
/// GitHub code scanning alert.
fn manifest_key_column(
    snippets: &mut SourceSnippetCache,
    path: &Path,
    line: u32,
    name: &str,
) -> u32 {
    snippets
        .line(path, line)
        .and_then(|text| text.find(&format!("\"{name}\"")))
        .and_then(|offset| u32::try_from(offset).ok())
        .map_or(1, |offset| offset.saturating_add(1))
}

/// The manifest coordinates `manifest_key_column` needs from a dependency.
fn dep_key(dep: &UnusedDependency) -> (&Path, u32, &str) {
    (dep.path.as_path(), dep.line, dep.package_name.as_str())
}

/// Resolve one manifest column per finding up front, so the result closure that
/// needs them does not have to borrow the snippet cache it reads.
fn manifest_key_columns<'a, T: 'a>(
    findings: &'a [T],
    snippets: &mut SourceSnippetCache,
    key_of: impl Fn(&'a T) -> (&'a Path, u32, &'a str),
) -> std::vec::IntoIter<u32> {
    findings
        .iter()
        .map(|finding| {
            let (path, line, name) = key_of(finding);
            manifest_key_column(snippets, path, line, name)
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Extract SARIF fields for an unused dependency.
fn sarif_dep_fields(
    dep: &UnusedDependency,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    section: &str,
    col: u32,
) -> SarifFields {
    let workspace_context = if dep.used_in_workspaces.is_empty() {
        String::new()
    } else {
        let workspaces = dep
            .used_in_workspaces
            .iter()
            .map(|path| relative_uri(path, root))
            .collect::<Vec<_>>()
            .join(", ");
        format!("; imported in other workspaces: {workspaces}")
    };
    SarifFields {
        rule_id,
        level,
        message: format!(
            "Package '{}' is in {} but never imported{}",
            dep.package_name, section, workspace_context
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, col))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

/// Extract SARIF fields for an unused enum or class member.
fn sarif_member_fields(
    member: &UnusedMember,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    kind: &str,
) -> SarifFields {
    SarifFields {
        rule_id,
        level,
        message: format!(
            "{} member '{}.{}' is never referenced",
            kind, member.parent_name, member.member_name
        ),
        uri: relative_uri(&member.path, root),
        region: Some((member.line, member.col + 1)),
        source_path: Some(member.path.clone()),
        properties: None,
    }
}

/// Append the degraded-parse caveat to a SARIF result message, so a reviewer
/// reading an annotation in CI sees the same qualifier the JSON envelope and
/// the human report carry. Byte-identical when the finding has no caveat.
fn with_caveats(mut fields: SarifFields, caveats: &[ReachabilityCaveat]) -> SarifFields {
    if let Some(labels) = caveat_labels(caveats) {
        fields.message.push_str(" (caveat: ");
        fields.message.push_str(&labels);
        fields.message.push(')');
    }
    fields
}

fn sarif_unused_file_fields(file: &UnusedFile, root: &Path, level: &'static str) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-file",
        level,
        message: "File is not reachable from any entry point".to_string(),
        uri: relative_uri(&file.path, root),
        region: None,
        source_path: None,
        properties: None,
    }
}

fn sarif_type_only_dep_fields(
    dep: &TypeOnlyDependency,
    root: &Path,
    level: &'static str,
    col: u32,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/type-only-dependency",
        level,
        message: format!(
            "Package '{}' is only imported via type-only imports (consider moving to devDependencies)",
            dep.package_name
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, col))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

fn sarif_test_only_dep_fields(
    dep: &TestOnlyDependency,
    root: &Path,
    level: &'static str,
    col: u32,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/test-only-dependency",
        level,
        message: format!(
            "Package '{}' is only imported by test files (consider moving to devDependencies)",
            dep.package_name
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, col))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

fn sarif_dev_dep_in_prod_fields(
    dep: &DevDependencyInProduction,
    root: &Path,
    level: &'static str,
    col: u32,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/dev-dependency-in-production",
        level,
        message: format!(
            "devDependency '{}' is imported by production code at runtime (consider moving to dependencies)",
            dep.package_name
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, col))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

fn sarif_unresolved_import_fields(
    import: &UnresolvedImport,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unresolved-import",
        level,
        message: format!("Import '{}' could not be resolved", import.specifier),
        uri: relative_uri(&import.path, root),
        region: Some((import.line, import.col + 1)),
        source_path: Some(import.path.clone()),
        properties: None,
    }
}

fn sarif_circular_dep_fields(
    cycle: &CircularDependency,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let chain: Vec<String> = cycle.files.iter().map(|p| relative_uri(p, root)).collect();
    let mut display_chain = chain.clone();
    if let Some(first) = chain.first() {
        display_chain.push(first.clone());
    }
    let first_uri = chain.first().map_or_else(String::new, Clone::clone);
    let first_path = cycle.files.first().cloned();
    SarifFields {
        rule_id: "fallow/circular-dependency",
        level,
        message: format!(
            "Circular dependency{}: {}",
            if cycle.is_cross_package {
                " (cross-package)"
            } else {
                ""
            },
            display_chain.join(" \u{2192} ")
        ),
        uri: first_uri,
        region: if cycle.line > 0 {
            Some((cycle.line, cycle.col + 1))
        } else {
            None
        },
        source_path: (cycle.line > 0).then_some(first_path).flatten(),
        properties: None,
    }
}

fn sarif_re_export_cycle_fields(
    cycle: &fallow_types::results::ReExportCycle,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let chain: Vec<String> = cycle.files.iter().map(|p| relative_uri(p, root)).collect();
    let first_uri = chain.first().map_or_else(String::new, Clone::clone);
    let first_path = cycle.files.first().cloned();
    let kind_tag = match cycle.kind {
        fallow_types::results::ReExportCycleKind::SelfLoop => " (self-loop)",
        fallow_types::results::ReExportCycleKind::MultiNode => "",
    };
    SarifFields {
        rule_id: "fallow/re-export-cycle",
        level,
        message: format!("Re-export cycle{}: {}", kind_tag, chain.join(" <-> ")),
        uri: first_uri,
        region: None,
        source_path: first_path,
        properties: None,
    }
}

fn sarif_boundary_violation_fields(
    violation: &BoundaryViolation,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let from_uri = relative_uri(&violation.from_path, root);
    let to_uri = relative_uri(&violation.to_path, root);
    SarifFields {
        rule_id: "fallow/boundary-violation",
        level,
        message: format!(
            "Import from zone '{}' to zone '{}' is not allowed ({})",
            violation.from_zone, violation.to_zone, to_uri,
        ),
        uri: from_uri,
        region: if violation.line > 0 {
            Some((violation.line, violation.col + 1))
        } else {
            None
        },
        source_path: (violation.line > 0).then(|| violation.from_path.clone()),
        properties: None,
    }
}

fn sarif_boundary_coverage_fields(
    violation: &BoundaryCoverageViolation,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/boundary-coverage",
        level,
        message: "File does not match any configured architecture boundary zone".to_string(),
        uri: relative_uri(&violation.path, root),
        region: Some((violation.line, violation.col + 1)),
        source_path: Some(violation.path.clone()),
        properties: None,
    }
}

fn sarif_boundary_call_fields(
    violation: &BoundaryCallViolation,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/boundary-call-violation",
        level,
        message: format!(
            "Call to `{}` matches forbidden pattern `{}` in zone '{}'",
            violation.callee, violation.pattern, violation.zone
        ),
        uri: relative_uri(&violation.path, root),
        region: Some((violation.line, violation.col + 1)),
        source_path: Some(violation.path.clone()),
        properties: None,
    }
}

fn sarif_policy_violation_fields(violation: &PolicyViolation, root: &Path) -> SarifFields {
    let level = match violation.severity {
        PolicyViolationSeverity::Error => "error",
        PolicyViolationSeverity::Warn => "warning",
    };
    let message = match &violation.message {
        Some(message) => format!(
            "Policy violation `{}/{}`: `{}` is banned. {message}",
            violation.pack, violation.rule_id, violation.matched
        ),
        None => format!(
            "Policy violation `{}/{}`: `{}` is banned",
            violation.pack, violation.rule_id, violation.matched
        ),
    };
    SarifFields {
        rule_id: "fallow/policy-violation",
        level,
        message,
        uri: relative_uri(&violation.path, root),
        region: Some((violation.line, violation.col + 1)),
        source_path: Some(violation.path.clone()),
        // The SARIF rule id is the static `fallow/policy-violation`; the
        // per-rule policy identity rides in properties so code-scanning
        // consumers can group or filter per pack rule without parsing the
        // message. Dynamic per-rule SARIF rule synthesis is a tracked
        // follow-up shared with boundary zone rules.
        properties: Some(serde_json::json!({
            "policyRule": format!("{}/{}", violation.pack, violation.rule_id),
        })),
    }
}

fn sarif_invalid_client_export_fields(
    export: &InvalidClientExport,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/invalid-client-export",
        level,
        message: format!(
            "Export '{}' is not allowed in a \"{}\" file (Next.js server-only / route-config name)",
            export.export_name, export.directive
        ),
        uri: relative_uri(&export.path, root),
        region: Some((export.line, export.col + 1)),
        source_path: Some(export.path.clone()),
        properties: None,
    }
}

fn sarif_mixed_client_server_barrel_fields(
    barrel: &MixedClientServerBarrel,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/mixed-client-server-barrel",
        level,
        message: format!(
            "Barrel re-exports both a \"use client\" module ('{}') and a server-only module ('{}'); one import drags the other's directive across the boundary",
            barrel.client_origin, barrel.server_origin
        ),
        uri: relative_uri(&barrel.path, root),
        region: Some((barrel.line, barrel.col + 1)),
        source_path: Some(barrel.path.clone()),
        properties: None,
    }
}

fn sarif_misplaced_directive_fields(
    directive_site: &MisplacedDirective,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/misplaced-directive",
        level,
        message: format!(
            "Directive \"{}\" is not in the leading position, so the RSC bundler ignores it; move it to the top of the file",
            directive_site.directive
        ),
        uri: relative_uri(&directive_site.path, root),
        region: Some((directive_site.line, directive_site.col + 1)),
        source_path: Some(directive_site.path.clone()),
        properties: None,
    }
}

fn sarif_unprovided_inject_fields(
    inject: &UnprovidedInject,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unprovided-inject",
        level,
        message: format!(
            "inject(\"{}\") has no matching provide(\"{}\") in this project; at runtime it returns undefined; provide the key or remove this inject",
            inject.key_name, inject.key_name
        ),
        uri: relative_uri(&inject.path, root),
        region: Some((inject.line, inject.col + 1)),
        source_path: Some(inject.path.clone()),
        properties: None,
    }
}

fn sarif_unrendered_component_fields(
    component: &UnrenderedComponent,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unrendered-component",
        level,
        message: format!(
            "component \"{}\" is reachable but rendered nowhere in this project; render it somewhere or remove it",
            component.component_name
        ),
        uri: relative_uri(&component.path, root),
        region: Some((component.line, component.col + 1)),
        source_path: Some(component.path.clone()),
        properties: None,
    }
}

fn sarif_unused_component_prop_fields(
    prop: &UnusedComponentProp,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-component-prop",
        level,
        message: format!(
            "prop \"{}\" is declared but referenced nowhere inside component \"{}\"; remove it or use it",
            prop.prop_name, prop.component_name
        ),
        uri: relative_uri(&prop.path, root),
        region: Some((prop.line, prop.col + 1)),
        source_path: Some(prop.path.clone()),
        properties: None,
    }
}

fn sarif_unused_component_emit_fields(
    emit: &UnusedComponentEmit,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-component-emit",
        level,
        message: format!(
            "emit \"{}\" is declared but emitted nowhere inside component \"{}\"; remove it or emit it",
            emit.emit_name, emit.component_name
        ),
        uri: relative_uri(&emit.path, root),
        region: Some((emit.line, emit.col + 1)),
        source_path: Some(emit.path.clone()),
        properties: None,
    }
}

fn sarif_unused_svelte_event_fields(
    event: &UnusedSvelteEvent,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-svelte-event",
        level,
        message: format!(
            "event \"{}\" is dispatched by component \"{}\" but listened to nowhere in the project; remove it or listen for it",
            event.event_name, event.component_name
        ),
        uri: relative_uri(&event.path, root),
        region: Some((event.line, event.col + 1)),
        source_path: Some(event.path.clone()),
        properties: None,
    }
}

fn sarif_unused_component_input_fields(
    input: &UnusedComponentInput,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-component-input",
        level,
        message: format!(
            "input \"{}\" is declared but read nowhere inside component \"{}\"; remove it or use it",
            input.input_name, input.component_name
        ),
        uri: relative_uri(&input.path, root),
        region: Some((input.line, input.col + 1)),
        source_path: Some(input.path.clone()),
        properties: None,
    }
}

fn sarif_unused_component_output_fields(
    output: &UnusedComponentOutput,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-component-output",
        level,
        message: format!(
            "output \"{}\" is declared but emitted nowhere inside component \"{}\"; remove it or emit it",
            output.output_name, output.component_name
        ),
        uri: relative_uri(&output.path, root),
        region: Some((output.line, output.col + 1)),
        source_path: Some(output.path.clone()),
        properties: None,
    }
}

fn sarif_unused_server_action_fields(
    action: &UnusedServerAction,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-server-action",
        level,
        message: format!(
            "server action \"{}\" is exported from a \"use server\" file but no code in this project references it; wire it to a consumer or remove it",
            action.action_name
        ),
        uri: relative_uri(&action.path, root),
        region: Some((action.line, action.col + 1)),
        source_path: Some(action.path.clone()),
        properties: None,
    }
}

fn sarif_unused_load_data_key_fields(
    key: &fallow_types::results::UnusedLoadDataKey,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-load-data-key",
        level,
        message: format!(
            "load() return key \"{}\" is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>); delete the key or wire a consumer",
            key.key_name
        ),
        uri: relative_uri(&key.path, root),
        region: Some((key.line, key.col + 1)),
        source_path: Some(key.path.clone()),
        properties: None,
    }
}

fn sarif_prop_drilling_fields(
    chain: &PropDrillingChain,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    // Anchor at the source hop (the prop owner). Path / line come from the first
    // hop; the message names the depth and the consumer at the chain tail.
    let source = chain.hops.first();
    let consumer = chain.hops.last();
    let (path, line) = source.map_or((std::path::PathBuf::new(), 1), |h| (h.file.clone(), h.line));
    let consumer_name = consumer.map_or("a distant component", |h| h.component.as_str());
    SarifFields {
        rule_id: "fallow/prop-drilling",
        level,
        message: format!(
            "prop \"{}\" is forwarded unchanged through {} component(s) before \"{}\" consumes it; colocate, lift to context, or compose",
            chain.prop, chain.depth, consumer_name
        ),
        uri: relative_uri(&path, root),
        region: Some((line, 1)),
        source_path: Some(path),
        properties: None,
    }
}

fn sarif_thin_wrapper_fields(
    wrapper: &ThinWrapper,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/thin-wrapper",
        level,
        message: format!(
            "\"{}\" is a thin wrapper: its whole body forwards props to \"{}\"; inline it at call sites or delete it",
            wrapper.component, wrapper.child_component
        ),
        uri: relative_uri(&wrapper.file, root),
        region: Some((wrapper.line, 1)),
        source_path: Some(wrapper.file.clone()),
        properties: None,
    }
}

fn sarif_duplicate_prop_shape_fields(
    shape: &DuplicatePropShape,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/duplicate-prop-shape",
        level,
        message: format!(
            "\"{}\" shares an identical prop shape {{{}}} with {} other component(s); extract a shared Props type or base component",
            shape.component,
            shape.shape.join(", "),
            shape.group_size.saturating_sub(1)
        ),
        uri: relative_uri(&shape.file, root),
        region: Some((shape.line, 1)),
        source_path: Some(shape.file.clone()),
        properties: None,
    }
}

fn sarif_route_collision_fields(
    collision: &RouteCollision,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/route-collision",
        level,
        message: format!(
            "Route file resolves to '{}', which is also owned by {} other file(s); Next.js fails the build because a URL can have only one owner",
            collision.url,
            collision.conflicting_paths.len()
        ),
        uri: relative_uri(&collision.path, root),
        region: Some((collision.line, collision.col + 1)),
        source_path: Some(collision.path.clone()),
        properties: None,
    }
}

fn sarif_dynamic_segment_name_conflict_fields(
    conflict: &DynamicSegmentNameConflict,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/dynamic-segment-name-conflict",
        level,
        message: format!(
            "Dynamic segments at '{}' use different slug names ({}); Next.js requires one consistent name per dynamic path",
            conflict.position,
            conflict.conflicting_segments.join(", ")
        ),
        uri: relative_uri(&conflict.path, root),
        region: Some((conflict.line, conflict.col + 1)),
        source_path: Some(conflict.path.clone()),
        properties: None,
    }
}

fn sarif_stale_suppression_fields(
    suppression: &StaleSuppression,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: if suppression.missing_reason {
            "fallow/missing-suppression-reason"
        } else {
            "fallow/stale-suppression"
        },
        level,
        message: suppression.display_message(),
        uri: relative_uri(&suppression.path, root),
        region: Some((suppression.line, suppression.col + 1)),
        source_path: Some(suppression.path.clone()),
        properties: None,
    }
}

fn stale_suppression_severity(suppression: &StaleSuppression, rules: &RulesConfig) -> Severity {
    if suppression.missing_reason {
        rules.require_suppression_reason
    } else {
        rules.stale_suppressions
    }
}

fn sarif_unused_catalog_entry_fields(
    entry: &UnusedCatalogEntryFinding,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let entry = &entry.entry;
    let message = if entry.catalog_name == "default" {
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
    SarifFields {
        rule_id: "fallow/unused-catalog-entry",
        level,
        message,
        uri: relative_uri(&entry.path, root),
        region: Some((entry.line, 1)),
        source_path: Some(entry.path.clone()),
        properties: None,
    }
}

fn sarif_unused_dependency_override_fields(
    finding: &UnusedDependencyOverrideFinding,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let finding = &finding.entry;
    let mut message = format!(
        "Override `{}` forces version `{}` but `{}` is not declared by any workspace package or resolved in the lockfile",
        finding.raw_key, finding.version_range, finding.target_package,
    );
    if let Some(hint) = &finding.hint {
        use std::fmt::Write as _;
        let _ = write!(message, " ({hint})");
    }
    SarifFields {
        rule_id: "fallow/unused-dependency-override",
        level,
        message,
        uri: relative_uri(&finding.path, root),
        region: Some((finding.line, 1)),
        source_path: Some(finding.path.clone()),
        properties: None,
    }
}

fn sarif_misconfigured_dependency_override_fields(
    finding: &MisconfiguredDependencyOverrideFinding,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let finding = &finding.entry;
    let message = format!(
        "Override `{}` -> `{}` is malformed: {}",
        finding.raw_key,
        finding.raw_value,
        finding.reason.describe(),
    );
    SarifFields {
        rule_id: "fallow/misconfigured-dependency-override",
        level,
        message,
        uri: relative_uri(&finding.path, root),
        region: Some((finding.line, 1)),
        source_path: Some(finding.path.clone()),
        properties: None,
    }
}

fn sarif_unresolved_catalog_reference_fields(
    finding: &UnresolvedCatalogReferenceFinding,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let finding = &finding.reference;
    let catalog_phrase = if finding.catalog_name == "default" {
        "the default catalog".to_string()
    } else {
        format!("catalog '{}'", finding.catalog_name)
    };
    let mut message = format!(
        "Package '{}' is referenced via `catalog:{}` but {} does not declare it",
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
            message,
            " (available in: {})",
            finding.available_in_catalogs.join(", ")
        );
    }
    SarifFields {
        rule_id: "fallow/unresolved-catalog-reference",
        level,
        message,
        uri: relative_uri(&finding.path, root),
        region: Some((finding.line, 1)),
        source_path: Some(finding.path.clone()),
        properties: None,
    }
}

fn sarif_empty_catalog_group_fields(
    group: &EmptyCatalogGroupFinding,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let group = &group.group;
    SarifFields {
        rule_id: "fallow/empty-catalog-group",
        level,
        message: format!("Catalog group '{}' has no entries", group.catalog_name),
        uri: relative_uri(&group.path, root),
        region: Some((group.line, 1)),
        source_path: Some(group.path.clone()),
        properties: None,
    }
}

/// Unlisted deps fan out to one SARIF result per import site, so they do not
/// fit `push_sarif_results`. Keep the nested-loop shape in its own helper.
fn push_sarif_unlisted_deps(
    sarif_results: &mut Vec<serde_json::Value>,
    deps: &[UnlistedDependencyFinding],
    root: &Path,
    rule: Severity,
    snippets: &mut SourceSnippetCache,
) {
    for entry in deps {
        let level = finding_level(entry, rule);
        let dep = &entry.dep;
        for site in &dep.imported_from {
            let uri = relative_uri(&site.path, root);
            let source_snippet = snippets.line(&site.path, site.line);
            sarif_results.push(sarif_result_with_snippet(
                "fallow/unlisted-dependency",
                level,
                &format!(
                    "Package '{}' is imported but not listed in package.json",
                    dep.package_name
                ),
                &uri,
                Some((site.line, site.col + 1)),
                source_snippet.as_deref(),
            ));
        }
    }
}

/// Duplicate exports fan out to one SARIF result per location
/// (SARIF 2.1.0 section 3.27.12), so they do not fit `push_sarif_results`.
fn push_sarif_duplicate_exports(
    sarif_results: &mut Vec<serde_json::Value>,
    dups: &[DuplicateExportFinding],
    root: &Path,
    rule: Severity,
    snippets: &mut SourceSnippetCache,
) {
    for dup in dups {
        let level = finding_level(dup, rule);
        let dup = &dup.export;
        for loc in &dup.locations {
            let uri = relative_uri(&loc.path, root);
            let source_snippet = snippets.line(&loc.path, loc.line);
            sarif_results.push(sarif_result_with_snippet(
                "fallow/duplicate-export",
                level,
                &format!("Export '{}' appears in multiple modules", dup.export_name),
                &uri,
                Some((loc.line, loc.col + 1)),
                source_snippet.as_deref(),
            ));
        }
    }
}

/// Build the SARIF rules list from the current rules configuration.
fn build_sarif_rules(
    rules: &RulesConfig,
    rule_builder: &dyn Fn(&str, &str, &str) -> serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut sarif_rules = Vec::new();
    for contract in issue_output_contracts() {
        for rule_id in contract.sarif_rule_ids {
            let severity = sarif_rule_severity(rules, contract.code, &rule_id);
            let description = issue_sarif_rule_description(&rule_id).unwrap_or_else(|| {
                panic!("dead-code SARIF rule {rule_id} is missing issue metadata")
            });
            sarif_rules.push(rule_builder(
                &rule_id,
                description,
                configured_sarif_level(severity),
            ));
        }
    }
    sarif_rules
}

fn sarif_rule_severity(rules: &RulesConfig, issue_code: &str, rule_id: &str) -> Severity {
    if rule_id == "fallow/missing-suppression-reason" {
        return rules.require_suppression_reason;
    }
    dead_code_rule_severity(rules, issue_code)
        .unwrap_or_else(|| panic!("dead-code SARIF rule {rule_id} has no severity mapping"))
}

fn dead_code_rule_severity(rules: &RulesConfig, issue_code: &str) -> Option<Severity> {
    let severity = match issue_code {
        "unused-file" => rules.unused_files,
        "unused-export" => rules.unused_exports,
        "unused-type" => rules.unused_types,
        "private-type-leak" => rules.private_type_leaks,
        "deprecated-export-in-use" => rules.deprecated_exports_in_use,
        "unused-dependency" => rules.unused_dependencies,
        "unused-dev-dependency" => rules.unused_dev_dependencies,
        "unused-optional-dependency" => rules.unused_optional_dependencies,
        "type-only-dependency" => rules.type_only_dependencies,
        "test-only-dependency" => rules.test_only_dependencies,
        "dev-dependency-in-production" => rules.dev_dependencies_in_production,
        "unused-enum-member" => rules.unused_enum_members,
        "unused-class-member" => rules.unused_class_members,
        "unused-store-member" => rules.unused_store_members,
        "unresolved-import" => rules.unresolved_imports,
        "unlisted-dependency" => rules.unlisted_dependencies,
        "duplicate-export" => rules.duplicate_exports,
        "circular-dependency" => rules.circular_dependencies,
        "re-export-cycle" => rules.re_export_cycle,
        "boundary-violation" | "boundary-coverage" | "boundary-call-violation" => {
            rules.boundary_violation
        }
        "policy-violation" => rules.policy_violation,
        "invalid-client-export" => rules.invalid_client_export,
        "mixed-client-server-barrel" => rules.mixed_client_server_barrel,
        "misplaced-directive" => rules.misplaced_directive,
        "unprovided-inject" => rules.unprovided_injects,
        "unrendered-component" => rules.unrendered_components,
        "unused-component-prop" => rules.unused_component_props,
        "unused-component-emit" => rules.unused_component_emits,
        "unused-component-input" => rules.unused_component_inputs,
        "unused-component-output" => rules.unused_component_outputs,
        "unused-svelte-event" => rules.unused_svelte_events,
        "unused-server-action" => rules.unused_server_actions,
        "unused-load-data-key" => rules.unused_load_data_keys,
        "prop-drilling" => non_gating_severity(rules.prop_drilling),
        "thin-wrapper" => non_gating_severity(rules.thin_wrapper),
        "duplicate-prop-shape" => non_gating_severity(rules.duplicate_prop_shape),
        "route-collision" => rules.route_collision,
        "dynamic-segment-name-conflict" => rules.dynamic_segment_name_conflict,
        "stale-suppression" => rules.stale_suppressions,
        "unused-catalog-entry" => rules.unused_catalog_entries,
        "empty-catalog-group" => rules.empty_catalog_groups,
        "unresolved-catalog-reference" => rules.unresolved_catalog_references,
        "unused-dependency-override" => rules.unused_dependency_overrides,
        "misconfigured-dependency-override" => rules.misconfigured_dependency_overrides,
        _ => return None,
    };
    Some(severity)
}

/// Builds the complete SARIF `run` value for a dead-code analysis.
///
/// Emits one SARIF result per finding across every dead-code issue kind,
/// mapping each configured rule severity to a SARIF level. `rule_builder`
/// constructs the tool-driver rule object for a `(rule id, name, help URI)`
/// triple so the caller controls rule metadata.
#[must_use]
pub fn build_dead_code_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    rule_builder: &dyn Fn(&str, &str, &str) -> serde_json::Value,
) -> serde_json::Value {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::with_root(root);
    let ctx = SarifCtx {
        results,
        root,
        rules,
    };

    push_primary_dead_code_sarif_results(&mut sarif_results, &ctx, &mut snippets);
    push_dependency_sarif_results(&mut sarif_results, &ctx, &mut snippets);
    push_member_sarif_results(&mut sarif_results, &ctx, &mut snippets);
    push_sarif_results(
        &mut sarif_results,
        &results.unresolved_imports,
        &mut snippets,
        |i| {
            sarif_unresolved_import_fields(
                &i.import,
                root,
                finding_level(i, rules.unresolved_imports),
            )
        },
    );
    push_misc_sarif_results(&mut sarif_results, &ctx, &mut snippets);
    push_graph_sarif_results(&mut sarif_results, &ctx, &mut snippets);
    push_catalog_sarif_results(&mut sarif_results, &ctx, &mut snippets);

    let sarif_rules = build_sarif_rules(rules, rule_builder);
    sarif_document(&sarif_results, &sarif_rules)
}

fn push_primary_dead_code_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.unused_files, snippets, |finding| {
        with_caveats(
            sarif_unused_file_fields(
                &finding.file,
                root,
                finding_level(finding, rules.unused_files),
            ),
            &finding.reachability_caveats,
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unused_exports,
        snippets,
        |finding| {
            with_caveats(
                sarif_export_fields(
                    &finding.export,
                    root,
                    "fallow/unused-export",
                    finding_level(finding, rules.unused_exports),
                    "Export",
                    "Re-export",
                ),
                &finding.reachability_caveats,
            )
        },
    );
    push_sarif_results(sarif_results, &results.unused_types, snippets, |finding| {
        with_caveats(
            sarif_export_fields(
                &finding.export,
                root,
                "fallow/unused-type",
                finding_level(finding, rules.unused_types),
                "Type export",
                "Type re-export",
            ),
            &finding.reachability_caveats,
        )
    });
    push_sarif_results(
        sarif_results,
        &results.private_type_leaks,
        snippets,
        |finding| {
            sarif_private_type_leak_fields(
                &finding.leak,
                root,
                finding_level(finding, rules.private_type_leaks),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.deprecated_exports_in_use,
        snippets,
        |finding| {
            sarif_deprecated_export_fields(
                &finding.export,
                root,
                finding_level(finding, rules.deprecated_exports_in_use),
            )
        },
    );
}

fn sarif_document(
    sarif_results: &[serde_json::Value],
    sarif_rules: &[serde_json::Value],
) -> serde_json::Value {
    build_sarif_document(SarifDocumentInput {
        results: sarif_results,
        rules: sarif_rules,
        tool_version: env!("CARGO_PKG_VERSION"),
    })
}

fn push_dependency_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    push_unused_dependency_sarif_results(sarif_results, ctx, snippets);
    push_classified_dependency_sarif_results(sarif_results, ctx, snippets);
}

/// Push SARIF results for unused runtime, dev, and optional dependencies.
fn push_unused_dependency_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    let mut columns =
        manifest_key_columns(&results.unused_dependencies, snippets, |f| dep_key(&f.dep));
    push_sarif_results(sarif_results, &results.unused_dependencies, snippets, |d| {
        with_caveats(
            sarif_dep_fields(
                &d.dep,
                root,
                "fallow/unused-dependency",
                finding_level(d, rules.unused_dependencies),
                "dependencies",
                columns.next().unwrap_or(1),
            ),
            &d.reachability_caveats,
        )
    });
    let mut columns = manifest_key_columns(&results.unused_dev_dependencies, snippets, |f| {
        dep_key(&f.dep)
    });
    push_sarif_results(
        sarif_results,
        &results.unused_dev_dependencies,
        snippets,
        |d| {
            with_caveats(
                sarif_dep_fields(
                    &d.dep,
                    root,
                    "fallow/unused-dev-dependency",
                    finding_level(d, rules.unused_dev_dependencies),
                    "devDependencies",
                    columns.next().unwrap_or(1),
                ),
                &d.reachability_caveats,
            )
        },
    );
    let mut columns = manifest_key_columns(&results.unused_optional_dependencies, snippets, |f| {
        dep_key(&f.dep)
    });
    push_sarif_results(
        sarif_results,
        &results.unused_optional_dependencies,
        snippets,
        |d| {
            with_caveats(
                sarif_dep_fields(
                    &d.dep,
                    root,
                    "fallow/unused-optional-dependency",
                    finding_level(d, rules.unused_optional_dependencies),
                    "optionalDependencies",
                    columns.next().unwrap_or(1),
                ),
                &d.reachability_caveats,
            )
        },
    );
}

/// Push SARIF results for type-only and test-only dependency misclassifications.
fn push_classified_dependency_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    let mut columns = manifest_key_columns(&results.type_only_dependencies, snippets, |f| {
        (
            f.dep.path.as_path(),
            f.dep.line,
            f.dep.package_name.as_str(),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.type_only_dependencies,
        snippets,
        |d| {
            sarif_type_only_dep_fields(
                &d.dep,
                root,
                finding_level(d, rules.type_only_dependencies),
                columns.next().unwrap_or(1),
            )
        },
    );
    let mut columns = manifest_key_columns(&results.test_only_dependencies, snippets, |f| {
        (
            f.dep.path.as_path(),
            f.dep.line,
            f.dep.package_name.as_str(),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.test_only_dependencies,
        snippets,
        |d| {
            sarif_test_only_dep_fields(
                &d.dep,
                root,
                finding_level(d, rules.test_only_dependencies),
                columns.next().unwrap_or(1),
            )
        },
    );
    let mut columns =
        manifest_key_columns(&results.dev_dependencies_in_production, snippets, |f| {
            (
                f.dep.path.as_path(),
                f.dep.line,
                f.dep.package_name.as_str(),
            )
        });
    push_sarif_results(
        sarif_results,
        &results.dev_dependencies_in_production,
        snippets,
        |d| {
            sarif_dev_dep_in_prod_fields(
                &d.dep,
                root,
                finding_level(d, rules.dev_dependencies_in_production),
                columns.next().unwrap_or(1),
            )
        },
    );
}

fn push_member_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.unused_enum_members, snippets, |m| {
        with_caveats(
            sarif_member_fields(
                &m.member,
                root,
                "fallow/unused-enum-member",
                finding_level(m, rules.unused_enum_members),
                "Enum",
            ),
            &m.reachability_caveats,
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unused_class_members,
        snippets,
        |m| {
            with_caveats(
                sarif_member_fields(
                    &m.member,
                    root,
                    "fallow/unused-class-member",
                    finding_level(m, rules.unused_class_members),
                    "Class",
                ),
                &m.reachability_caveats,
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_store_members,
        snippets,
        |m| {
            with_caveats(
                sarif_member_fields(
                    &m.member,
                    root,
                    "fallow/unused-store-member",
                    finding_level(m, rules.unused_store_members),
                    "Store",
                ),
                &m.reachability_caveats,
            )
        },
    );
}

fn push_misc_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    if !results.unlisted_dependencies.is_empty() {
        push_sarif_unlisted_deps(
            sarif_results,
            &results.unlisted_dependencies,
            root,
            rules.unlisted_dependencies,
            snippets,
        );
    }
    if !results.duplicate_exports.is_empty() {
        push_sarif_duplicate_exports(
            sarif_results,
            &results.duplicate_exports,
            root,
            rules.duplicate_exports,
            snippets,
        );
    }
}

/// Push the component-contract SARIF results (`unused-component-prop` and
/// `unused-component-emit`). Extracted from `push_graph_sarif_results` to keep
/// that function under the unit-size lint.
fn push_component_contract_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    push_component_member_sarif_results(sarif_results, ctx, snippets);
    push_component_framework_sarif_results(sarif_results, ctx, snippets);
    push_component_shape_sarif_results(sarif_results, ctx, snippets);
}

/// Push SARIF results for unused component props, emits, inputs, and outputs.
fn push_component_member_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.unused_component_props,
        snippets,
        |p| {
            sarif_unused_component_prop_fields(
                &p.prop,
                root,
                finding_level(p, rules.unused_component_props),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_component_emits,
        snippets,
        |e| {
            sarif_unused_component_emit_fields(
                &e.emit,
                root,
                finding_level(e, rules.unused_component_emits),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_component_inputs,
        snippets,
        |i| {
            sarif_unused_component_input_fields(
                &i.input,
                root,
                finding_level(i, rules.unused_component_inputs),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_component_outputs,
        snippets,
        |o| {
            sarif_unused_component_output_fields(
                &o.output,
                root,
                finding_level(o, rules.unused_component_outputs),
            )
        },
    );
}

/// Push SARIF results for Svelte events, server actions, and load-data keys.
fn push_component_framework_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.unused_svelte_events,
        snippets,
        |e| {
            sarif_unused_svelte_event_fields(
                &e.event,
                root,
                finding_level(e, rules.unused_svelte_events),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_server_actions,
        snippets,
        |a| {
            sarif_unused_server_action_fields(
                &a.action,
                root,
                finding_level(a, rules.unused_server_actions),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_load_data_keys,
        snippets,
        |k| {
            sarif_unused_load_data_key_fields(
                &k.key,
                root,
                finding_level(k, rules.unused_load_data_keys),
            )
        },
    );
}

/// Push SARIF results for prop drilling, thin wrappers, and duplicate prop shapes.
fn push_component_shape_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.prop_drilling_chains,
        snippets,
        |c| {
            sarif_prop_drilling_fields(
                &c.chain,
                root,
                severity_to_sarif_level(non_gating_severity(rules.prop_drilling)),
            )
        },
    );
    push_sarif_results(sarif_results, &results.thin_wrappers, snippets, |w| {
        sarif_thin_wrapper_fields(
            &w.wrapper,
            root,
            severity_to_sarif_level(non_gating_severity(rules.thin_wrapper)),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.duplicate_prop_shapes,
        snippets,
        |d| {
            sarif_duplicate_prop_shape_fields(
                &d.shape,
                root,
                severity_to_sarif_level(non_gating_severity(rules.duplicate_prop_shape)),
            )
        },
    );
}

fn push_graph_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    push_structure_sarif_results(sarif_results, ctx, snippets);
    push_framework_sarif_results(sarif_results, ctx, snippets);
    push_route_sarif_results(sarif_results, ctx, snippets);
    push_suppression_sarif_results(sarif_results, ctx, snippets);
}

fn push_structure_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    push_cycle_sarif_results(sarif_results, ctx, snippets);
    push_boundary_sarif_results(sarif_results, ctx, snippets);
}

/// Push SARIF results for circular dependencies and re-export cycles.
fn push_cycle_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.circular_dependencies,
        snippets,
        |c| {
            sarif_circular_dep_fields(
                &c.cycle,
                root,
                finding_level(c, rules.circular_dependencies),
            )
        },
    );
    push_sarif_results(sarif_results, &results.re_export_cycles, snippets, |c| {
        sarif_re_export_cycle_fields(&c.cycle, root, finding_level(c, rules.re_export_cycle))
    });
}

/// Push SARIF results for boundary violations, coverage, calls, and policy violations.
fn push_boundary_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.boundary_violations, snippets, |v| {
        sarif_boundary_violation_fields(
            &v.violation,
            root,
            finding_level(v, rules.boundary_violation),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.boundary_coverage_violations,
        snippets,
        |v| {
            sarif_boundary_coverage_fields(
                &v.violation,
                root,
                finding_level(v, rules.boundary_violation),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.boundary_call_violations,
        snippets,
        |v| {
            sarif_boundary_call_fields(
                &v.violation,
                root,
                finding_level(v, rules.boundary_violation),
            )
        },
    );
    push_sarif_results(sarif_results, &results.policy_violations, snippets, |v| {
        sarif_policy_violation_fields(&v.violation, root)
    });
}

fn push_framework_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    push_framework_boundary_sarif_results(sarif_results, ctx, snippets);
    push_component_contract_sarif_results(sarif_results, ctx, snippets);
}

/// Push SARIF results for client exports, barrels, directives, injects, and unrendered components.
fn push_framework_boundary_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.invalid_client_exports,
        snippets,
        |e| {
            sarif_invalid_client_export_fields(
                &e.export,
                root,
                finding_level(e, rules.invalid_client_export),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.mixed_client_server_barrels,
        snippets,
        |b| {
            sarif_mixed_client_server_barrel_fields(
                &b.barrel,
                root,
                finding_level(b, rules.mixed_client_server_barrel),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.misplaced_directives,
        snippets,
        |d| {
            sarif_misplaced_directive_fields(
                &d.directive_site,
                root,
                finding_level(d, rules.misplaced_directive),
            )
        },
    );
    push_framework_render_sarif_results(sarif_results, ctx, snippets);
}

fn push_framework_render_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.unprovided_injects, snippets, |i| {
        sarif_unprovided_inject_fields(&i.inject, root, finding_level(i, rules.unprovided_injects))
    });
    push_sarif_results(
        sarif_results,
        &results.unrendered_components,
        snippets,
        |c| {
            sarif_unrendered_component_fields(
                &c.component,
                root,
                finding_level(c, rules.unrendered_components),
            )
        },
    );
}

fn push_route_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.route_collisions, snippets, |c| {
        sarif_route_collision_fields(&c.collision, root, finding_level(c, rules.route_collision))
    });
    push_sarif_results(
        sarif_results,
        &results.dynamic_segment_name_conflicts,
        snippets,
        |c| {
            sarif_dynamic_segment_name_conflict_fields(
                &c.conflict,
                root,
                finding_level(c, rules.dynamic_segment_name_conflict),
            )
        },
    );
}

fn push_suppression_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.stale_suppressions, snippets, |s| {
        sarif_stale_suppression_fields(
            s,
            root,
            finding_level(s, stale_suppression_severity(s, rules)),
        )
    });
}

fn push_catalog_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    push_catalog_entry_sarif_results(sarif_results, ctx, snippets);
    push_dependency_override_sarif_results(sarif_results, ctx, snippets);
}

/// Push SARIF results for unused catalog entries, empty groups, and unresolved references.
fn push_catalog_entry_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.unused_catalog_entries,
        snippets,
        |e| {
            sarif_unused_catalog_entry_fields(
                e,
                root,
                finding_level(e, rules.unused_catalog_entries),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.empty_catalog_groups,
        snippets,
        |g| sarif_empty_catalog_group_fields(g, root, finding_level(g, rules.empty_catalog_groups)),
    );
    push_sarif_results(
        sarif_results,
        &results.unresolved_catalog_references,
        snippets,
        |f| {
            sarif_unresolved_catalog_reference_fields(
                f,
                root,
                finding_level(f, rules.unresolved_catalog_references),
            )
        },
    );
}

/// Push SARIF results for unused and misconfigured dependency overrides.
fn push_dependency_override_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.unused_dependency_overrides,
        snippets,
        |f| {
            sarif_unused_dependency_override_fields(
                f,
                root,
                finding_level(f, rules.unused_dependency_overrides),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.misconfigured_dependency_overrides,
        snippets,
        |f| {
            sarif_misconfigured_dependency_override_fields(
                f,
                root,
                finding_level(f, rules.misconfigured_dependency_overrides),
            )
        },
    );
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use fallow_config::RulesConfig;
    use fallow_types::results::AnalysisResults;

    use super::*;

    fn test_rule_builder(id: &str, description: &str, level: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "shortDescription": { "text": description },
            "defaultConfiguration": { "level": level }
        })
    }

    /// A SARIF consumer reads the message text, not the JSON envelope, so the
    /// degraded-parse caveat has to travel in the message. A clean finding
    /// keeps the previous text byte-for-byte.
    #[test]
    fn sarif_messages_name_the_degraded_parse_caveat() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: Path::new("/p/src/clean.ts").to_path_buf(),
            }));
        let mut flagged = UnusedFileFinding::with_actions(UnusedFile {
            path: Path::new("/p/src/orphan.ts").to_path_buf(),
        });
        flagged.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_files.push(flagged);

        // Every member array carries the same caveat off the same
        // reachability-free access walk, so all three have to reach the
        // message. Store members were rendered bare while the array itself was
        // stamped.
        let member = |parent: &str, name: &str, kind| UnusedMember {
            path: Path::new("/p/src/lib.ts").to_path_buf(),
            parent_name: parent.to_owned(),
            member_name: name.to_owned(),
            kind,
            line: 7,
            col: 2,
        };
        let mut enum_member = UnusedEnumMemberFinding::with_actions(member(
            "Mode",
            "Legacy",
            fallow_types::extract::MemberKind::EnumMember,
        ));
        enum_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_enum_members.push(enum_member);
        let mut class_member = UnusedClassMemberFinding::with_actions(member(
            "Widget",
            "render",
            fallow_types::extract::MemberKind::ClassMethod,
        ));
        class_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_class_members.push(class_member);
        let mut store_member = UnusedStoreMemberFinding::with_actions(member(
            "useCart",
            "subtotal",
            fallow_types::extract::MemberKind::StoreMember,
        ));
        store_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_store_members.push(store_member);

        let sarif = build_dead_code_sarif(
            &results,
            Path::new("/p"),
            &RulesConfig::default(),
            &test_rule_builder,
        );
        let messages: Vec<String> = sarif
            .pointer("/runs/0/results")
            .and_then(serde_json::Value::as_array)
            .expect("SARIF results")
            .iter()
            .filter_map(|entry| {
                entry
                    .pointer("/message/text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect();

        assert!(
            messages.contains(&"File is not reachable from any entry point".to_owned()),
            "a clean finding keeps its exact message: {messages:?}"
        );
        assert!(
            messages.contains(
                &"File is not reachable from any entry point (caveat: incomplete import graph)"
                    .to_owned()
            ),
            "a caveated finding names it in the message: {messages:?}"
        );
        for expected in [
            "Enum member 'Mode.Legacy' is never referenced (caveat: incomplete import graph)",
            "Class member 'Widget.render' is never referenced (caveat: incomplete import graph)",
            "Store member 'useCart.subtotal' is never referenced (caveat: incomplete import graph)",
        ] {
            assert!(
                messages.contains(&expected.to_owned()),
                "every member kind names the caveat: {messages:?}"
            );
        }
    }

    /// The reported defect: `npm init -y` writes a `package.json` whose
    /// dependency block can sit on one line, and `find_dep_line_in_json` also
    /// falls back to line 1 for a key it cannot locate. Every unused dependency
    /// then reported the same rule id, the same URI, and the same source
    /// snippet, which is the whole of a SARIF fingerprint, so GitHub code
    /// scanning showed one alert for all of them. CodeClimate never had the
    /// defect: it keys on the package name.
    #[test]
    fn two_dependencies_on_one_manifest_line_are_two_alerts() {
        let dir = tempfile::tempdir().expect("temporary project");
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"compact","dependencies":{"lodash":"^4.17.21","chalk":"^5.3.0"}}"#,
        )
        .expect("write manifest");

        let mut results = AnalysisResults::default();
        for name in ["lodash", "chalk"] {
            results
                .unused_dependencies
                .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: name.to_owned(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: root.join("package.json"),
                    line: 1,
                    used_in_workspaces: Vec::new(),
                }));
        }

        let sarif =
            build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);
        let entries = sarif
            .pointer("/runs/0/results")
            .and_then(serde_json::Value::as_array)
            .expect("SARIF results");

        let fingerprints = entries
            .iter()
            .map(|entry| {
                entry
                    .pointer("/partialFingerprints/tools.fallow.fingerprint~1v1")
                    .and_then(serde_json::Value::as_str)
                    .expect("fingerprint")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            fingerprints.len(),
            entries.len(),
            "two dependencies declared on one line are two alerts: {entries:#?}"
        );

        let columns = entries
            .iter()
            .map(|entry| {
                entry
                    .pointer("/locations/0/physicalLocation/region/startColumn")
                    .and_then(serde_json::Value::as_u64)
                    .expect("start column")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            columns.len(),
            entries.len(),
            "each dependency points at its own key in the manifest line: {entries:#?}"
        );
    }

    /// Why the column and not only the run-level uniqueness pass: that pass
    /// separates repeats by occurrence index, so a dependency's identity would
    /// depend on its siblings and fixing the first one would renumber, and
    /// close, the alert on the second. The column is the dependency's own
    /// position, so an unrelated dependency added above it moves neither.
    #[test]
    fn a_dependency_added_above_leaves_the_others_alert_alone() {
        let dir = tempfile::tempdir().expect("temporary project");
        let root = dir.path();
        let manifest = root.join("package.json");

        let chalk_fingerprint = |manifest_text: &str, chalk_line: u32| {
            std::fs::write(&manifest, manifest_text).expect("write manifest");
            let mut results = AnalysisResults::default();
            results
                .unused_dependencies
                .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: "chalk".to_owned(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: manifest.clone(),
                    line: chalk_line,
                    used_in_workspaces: Vec::new(),
                }));
            build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder)
                .pointer("/runs/0/results/0/partialFingerprints/tools.fallow.fingerprint~1v1")
                .and_then(serde_json::Value::as_str)
                .expect("chalk fingerprint")
                .to_owned()
        };

        let before = "{\n  \"dependencies\": {\n    \"chalk\": \"^5.3.0\"\n  }\n}\n";
        let after = "{\n  \"dependencies\": {\n    \"lodash\": \"^4.17.21\",\n    \"chalk\": \"^5.3.0\"\n  }\n}\n";

        assert_eq!(
            chalk_fingerprint(before, 3),
            chalk_fingerprint(after, 4),
            "a dependency declared above it must not move chalk's alert"
        );
    }

    /// `partialFingerprints` is the alert identity GitHub code scanning uses to
    /// carry a finding across runs. It is built from rule id plus location (or a
    /// normalized snippet), never from the message, so a finding that gains the
    /// caveat must keep its fingerprint. If the caveat ever reached the
    /// fingerprint inputs, every open alert on a degraded repository would close
    /// and reopen as new on the next scan.
    #[test]
    fn the_caveat_does_not_move_the_sarif_fingerprint() {
        let build = |caveated: bool| {
            let mut results = AnalysisResults::default();
            let mut finding = UnusedFileFinding::with_actions(UnusedFile {
                path: Path::new("/p/src/orphan.ts").to_path_buf(),
            });
            if caveated {
                finding.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
            }
            results.unused_files.push(finding);
            build_dead_code_sarif(
                &results,
                Path::new("/p"),
                &RulesConfig::default(),
                &test_rule_builder,
            )
        };

        let read = |sarif: &serde_json::Value, pointer: &str| {
            sarif
                .pointer("/runs/0/results")
                .and_then(serde_json::Value::as_array)
                .expect("SARIF results")
                .iter()
                .map(|entry| {
                    entry
                        .pointer(pointer)
                        .and_then(serde_json::Value::as_str)
                        .expect("SARIF field")
                        .to_owned()
                })
                .collect::<Vec<_>>()
        };

        let clean = build(false);
        let caveated = build(true);

        for key in [
            "/partialFingerprints/tools.fallow.fingerprint~1v1",
            "/partialFingerprints/primaryLocationLineHash~1v1",
        ] {
            assert_eq!(
                read(&clean, key),
                read(&caveated, key),
                "the caveat must not move {key}"
            );
        }

        assert_ne!(
            read(&clean, "/message/text"),
            read(&caveated, "/message/text"),
            "the guard is only meaningful while the message actually changed"
        );
    }

    #[test]
    fn sarif_rule_list_is_backed_by_issue_contracts() {
        let sarif = build_dead_code_sarif(
            &AnalysisResults::default(),
            Path::new("."),
            &RulesConfig::default(),
            &test_rule_builder,
        );
        let Some(rules) = sarif
            .pointer("/runs/0/tool/driver/rules")
            .and_then(serde_json::Value::as_array)
        else {
            panic!("SARIF document should contain driver rules");
        };

        let actual_ids = rules
            .iter()
            .filter_map(|rule| {
                rule.get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        let expected_ids = issue_output_contracts()
            .flat_map(|contract| contract.sarif_rule_ids)
            .collect::<BTreeSet<_>>();

        assert_eq!(actual_ids, expected_ids);

        for rule in rules {
            let id = rule
                .get("id")
                .and_then(serde_json::Value::as_str)
                .expect("SARIF rule should have id");
            let description = rule
                .pointer("/shortDescription/text")
                .and_then(serde_json::Value::as_str)
                .expect("SARIF rule should have short description");
            assert_eq!(
                description,
                issue_sarif_rule_description(id).expect("SARIF rule description should resolve")
            );
        }
    }
}
