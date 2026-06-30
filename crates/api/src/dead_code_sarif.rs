//! Shared dead-code SARIF output assembly.

use std::path::{Path, PathBuf};

use fallow_config::{RulesConfig, Severity};
use fallow_output::{
    SarifDocumentInput, SarifResultInput, build_sarif_document, build_sarif_result,
    issue_output_contract_by_code, normalize_uri,
};
use fallow_types::output_dead_code::*;
use fallow_types::results::{
    AnalysisResults, BoundaryCallViolation, BoundaryCoverageViolation, BoundaryViolation,
    CircularDependency, DuplicatePropShape, DynamicSegmentNameConflict, InvalidClientExport,
    MisplacedDirective, MixedClientServerBarrel, PolicyViolation, PolicyViolationSeverity,
    PrivateTypeLeak, PropDrillingChain, RouteCollision, StaleSuppression, TestOnlyDependency,
    ThinWrapper, TypeOnlyDependency, UnprovidedInject, UnrenderedComponent, UnresolvedImport,
    UnusedComponentEmit, UnusedComponentInput, UnusedComponentOutput, UnusedComponentProp,
    UnusedDependency, UnusedExport, UnusedFile, UnusedMember, UnusedServerAction,
    UnusedSvelteEvent,
};
use rustc_hash::FxHashMap;

fn relative_uri(path: &Path, root: &Path) -> String {
    normalize_uri(
        &path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}

/// Intermediate fields extracted from an issue for SARIF result construction.
struct SarifFields {
    rule_id: &'static str,
    level: &'static str,
    message: String,
    uri: String,
    region: Option<(u32, u32)>,
    source_path: Option<PathBuf>,
    properties: Option<serde_json::Value>,
}

#[derive(Default)]
struct SourceSnippetCache {
    files: FxHashMap<PathBuf, Vec<String>>,
}

impl SourceSnippetCache {
    fn line(&mut self, path: &Path, line: u32) -> Option<String> {
        if line == 0 {
            return None;
        }
        if !self.files.contains_key(path) {
            let lines = std::fs::read_to_string(path)
                .ok()
                .map(|source| source.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            self.files.insert(path.to_path_buf(), lines);
        }
        self.files
            .get(path)
            .and_then(|lines| lines.get(line.saturating_sub(1) as usize))
            .cloned()
    }
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

fn configured_sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::Error | Severity::Warn => severity_to_sarif_level(s),
        Severity::Off => "none",
    }
}

fn sarif_result_with_snippet(
    rule_id: &str,
    level: &str,
    message: &str,
    uri: &str,
    region: Option<(u32, u32)>,
    snippet: Option<&str>,
) -> serde_json::Value {
    build_sarif_result(SarifResultInput {
        rule_id,
        level,
        message,
        uri,
        region,
        snippet,
    })
}

/// Append SARIF results for a slice of items using a closure to extract fields.
fn push_sarif_results<T>(
    sarif_results: &mut Vec<serde_json::Value>,
    items: &[T],
    snippets: &mut SourceSnippetCache,
    mut extract: impl FnMut(&T) -> SarifFields,
) {
    for item in items {
        let fields = extract(item);
        let source_snippet = fields
            .source_path
            .as_deref()
            .zip(fields.region)
            .and_then(|(path, (line, _))| snippets.line(path, line));
        let mut result = sarif_result_with_snippet(
            fields.rule_id,
            fields.level,
            &fields.message,
            &fields.uri,
            fields.region,
            source_snippet.as_deref(),
        );
        if let Some(props) = fields.properties {
            result["properties"] = props;
        }
        sarif_results.push(result);
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

/// Extract SARIF fields for an unused dependency.
fn sarif_dep_fields(
    dep: &UnusedDependency,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    section: &str,
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
            Some((dep.line, 1))
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
            Some((dep.line, 1))
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
            Some((dep.line, 1))
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
        "Override `{}` forces version `{}` but `{}` is not declared by any workspace package or resolved in pnpm-lock.yaml",
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
    level: &'static str,
    snippets: &mut SourceSnippetCache,
) {
    for entry in deps {
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
    level: &'static str,
    snippets: &mut SourceSnippetCache,
) {
    for dup in dups {
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
    let mut specs = Vec::new();
    specs.extend(sarif_core_rule_specs(rules));
    specs.extend(sarif_dependency_rule_specs(rules));
    specs.extend(sarif_member_import_rule_specs(rules));
    specs.extend(sarif_graph_rule_specs(rules));
    specs.extend(sarif_workspace_rule_specs(rules));
    specs
        .into_iter()
        .map(|spec| {
            let rule_id = spec.rule_id();
            rule_builder(
                &rule_id,
                spec.description,
                configured_sarif_level(spec.severity),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SarifRuleSpec {
    issue_code: &'static str,
    rule_id_override: Option<&'static str>,
    description: &'static str,
    severity: Severity,
}

impl SarifRuleSpec {
    fn rule_id(self) -> String {
        let canonical_rule_id = format!("fallow/{}", self.issue_code);
        let Some(contract) = issue_output_contract_by_code(self.issue_code) else {
            panic!(
                "dead-code SARIF rule spec must reference an output contract: {}",
                self.issue_code
            );
        };
        let expected = self.rule_id_override.unwrap_or(canonical_rule_id.as_str());
        assert!(
            contract
                .sarif_rule_ids
                .iter()
                .any(|rule_id| rule_id == expected),
            "dead-code SARIF rule id {expected} is missing from issue contract {}",
            self.issue_code
        );
        expected.to_string()
    }
}

const fn sarif_rule_spec(
    issue_code: &'static str,
    description: &'static str,
    severity: Severity,
) -> SarifRuleSpec {
    SarifRuleSpec {
        issue_code,
        rule_id_override: None,
        description,
        severity,
    }
}

const fn sarif_rule_spec_with_id(
    issue_code: &'static str,
    rule_id: &'static str,
    description: &'static str,
    severity: Severity,
) -> SarifRuleSpec {
    SarifRuleSpec {
        issue_code,
        rule_id_override: Some(rule_id),
        description,
        severity,
    }
}

fn sarif_core_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    [
        sarif_rule_spec(
            "unused-file",
            "File is not reachable from any entry point",
            rules.unused_files,
        ),
        sarif_rule_spec(
            "unused-export",
            "Export is never imported",
            rules.unused_exports,
        ),
        sarif_rule_spec(
            "unused-type",
            "Type export is never imported",
            rules.unused_types,
        ),
        sarif_rule_spec(
            "private-type-leak",
            "Exported signature references a same-file private type",
            rules.private_type_leaks,
        ),
    ]
    .into()
}

fn sarif_dependency_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    [
        sarif_rule_spec(
            "unused-dependency",
            "Dependency listed but never imported",
            rules.unused_dependencies,
        ),
        sarif_rule_spec(
            "unused-dev-dependency",
            "Dev dependency listed but never imported",
            rules.unused_dev_dependencies,
        ),
        sarif_rule_spec(
            "unused-optional-dependency",
            "Optional dependency listed but never imported",
            rules.unused_optional_dependencies,
        ),
        sarif_rule_spec(
            "type-only-dependency",
            "Production dependency only used via type-only imports",
            rules.type_only_dependencies,
        ),
        sarif_rule_spec(
            "test-only-dependency",
            "Production dependency only imported by test files",
            rules.test_only_dependencies,
        ),
    ]
    .into()
}

fn sarif_member_import_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    [
        sarif_rule_spec(
            "unused-enum-member",
            "Enum member is never referenced",
            rules.unused_enum_members,
        ),
        sarif_rule_spec(
            "unused-class-member",
            "Class member is never referenced",
            rules.unused_class_members,
        ),
        sarif_rule_spec(
            "unused-store-member",
            "Store member is never referenced",
            rules.unused_store_members,
        ),
        sarif_rule_spec(
            "unresolved-import",
            "Import could not be resolved",
            rules.unresolved_imports,
        ),
        sarif_rule_spec(
            "unlisted-dependency",
            "Dependency used but not in package.json",
            rules.unlisted_dependencies,
        ),
        sarif_rule_spec(
            "duplicate-export",
            "Export name appears in multiple modules",
            rules.duplicate_exports,
        ),
    ]
    .into()
}

fn sarif_graph_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    let mut specs = sarif_cycle_rule_specs(rules);
    specs.extend(sarif_boundary_rule_specs(rules));
    specs.extend(sarif_framework_rule_specs(rules));
    specs.extend(sarif_component_rule_specs(rules));
    specs.push(sarif_rule_spec(
        "stale-suppression",
        "Suppression comment or tag no longer matches any issue",
        rules.stale_suppressions,
    ));
    specs.push(sarif_rule_spec_with_id(
        "stale-suppression",
        "fallow/missing-suppression-reason",
        "Suppression comment or tag is missing a required reason",
        rules.require_suppression_reason,
    ));
    specs
}

fn sarif_cycle_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    vec![
        sarif_rule_spec(
            "circular-dependency",
            "Circular dependency chain detected",
            rules.circular_dependencies,
        ),
        sarif_rule_spec(
            "re-export-cycle",
            "Two or more barrel files re-export from each other in a loop",
            rules.re_export_cycle,
        ),
    ]
}

fn sarif_boundary_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    vec![
        sarif_rule_spec(
            "boundary-violation",
            "Import crosses an architecture boundary",
            rules.boundary_violation,
        ),
        sarif_rule_spec(
            "boundary-coverage",
            "Source file matches no architecture boundary zone",
            rules.boundary_violation,
        ),
        sarif_rule_spec(
            "boundary-call-violation",
            "Zoned file calls a callee its zone forbids",
            rules.boundary_violation,
        ),
        sarif_rule_spec(
            "policy-violation",
            "Banned usage matched a rule-pack rule",
            rules.policy_violation,
        ),
    ]
}

fn sarif_framework_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    vec![
        sarif_rule_spec(
            "invalid-client-export",
            "\"use client\" file exports a server-only / route-config name",
            rules.invalid_client_export,
        ),
        sarif_rule_spec(
            "mixed-client-server-barrel",
            "Barrel re-exports both a \"use client\" module and a server-only module",
            rules.mixed_client_server_barrel,
        ),
        sarif_rule_spec(
            "misplaced-directive",
            "\"use client\" / \"use server\" directive is not in the leading position and is ignored",
            rules.misplaced_directive,
        ),
    ]
}

fn sarif_component_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    vec![
        sarif_rule_spec(
            "unprovided-inject",
            "A Vue inject / Svelte getContext whose key is provided nowhere in the project",
            rules.unprovided_injects,
        ),
        sarif_rule_spec(
            "unrendered-component",
            "A Vue / Svelte component reachable through a barrel but rendered nowhere in the project",
            rules.unrendered_components,
        ),
        sarif_rule_spec(
            "unused-component-prop",
            "A Vue <script setup> defineProps prop referenced nowhere inside its own component",
            rules.unused_component_props,
        ),
        sarif_rule_spec(
            "unused-component-emit",
            "A Vue <script setup> defineEmits event emitted nowhere inside its own component",
            rules.unused_component_emits,
        ),
        sarif_rule_spec(
            "unused-component-input",
            "An Angular @Input() / signal input() / model() input read nowhere inside its own component",
            rules.unused_component_inputs,
        ),
        sarif_rule_spec(
            "unused-component-output",
            "An Angular @Output() / signal output() output emitted nowhere inside its own component",
            rules.unused_component_outputs,
        ),
        sarif_rule_spec(
            "unused-svelte-event",
            "A Svelte component dispatching a createEventDispatcher event whose name is listened to nowhere in the project",
            rules.unused_svelte_events,
        ),
        sarif_rule_spec(
            "unused-server-action",
            "A Next.js Server Action exported from a \"use server\" file that no code in the project references",
            rules.unused_server_actions,
        ),
        sarif_rule_spec(
            "unused-load-data-key",
            "A SvelteKit load() return-object key that no consumer reads (sibling +page.svelte data.<key> or project-wide page.data.<key>)",
            rules.unused_load_data_keys,
        ),
        sarif_rule_spec(
            "prop-drilling",
            "A React/Preact prop forwarded unchanged through 3+ pass-through components to a distant consumer",
            rules.prop_drilling,
        ),
        sarif_rule_spec(
            "thin-wrapper",
            "A React/Preact component whose whole body is a single spread-forwarded child render (a candidate for inlining)",
            rules.thin_wrapper,
        ),
        sarif_rule_spec(
            "duplicate-prop-shape",
            "Three or more React/Preact components across two or more files declare an identical prop-name set (a missing shared Props type)",
            rules.duplicate_prop_shape,
        ),
        sarif_rule_spec(
            "route-collision",
            "Two or more Next.js App Router route files resolve to the same URL",
            rules.route_collision,
        ),
        sarif_rule_spec(
            "dynamic-segment-name-conflict",
            "Sibling Next.js dynamic route segments use different slug names at the same position",
            rules.dynamic_segment_name_conflict,
        ),
    ]
}

fn sarif_workspace_rule_specs(rules: &RulesConfig) -> Vec<SarifRuleSpec> {
    [
        sarif_rule_spec(
            "unused-catalog-entry",
            "pnpm catalog entry not referenced by any workspace package",
            rules.unused_catalog_entries,
        ),
        sarif_rule_spec(
            "empty-catalog-group",
            "pnpm named catalog group has no entries",
            rules.empty_catalog_groups,
        ),
        sarif_rule_spec(
            "unresolved-catalog-reference",
            "package.json catalog reference points at a catalog that does not declare the package",
            rules.unresolved_catalog_references,
        ),
        sarif_rule_spec(
            "unused-dependency-override",
            "pnpm dependency override target is not declared or lockfile-resolved",
            rules.unused_dependency_overrides,
        ),
        sarif_rule_spec(
            "misconfigured-dependency-override",
            "pnpm dependency override key or value is malformed",
            rules.misconfigured_dependency_overrides,
        ),
    ]
    .into()
}

#[must_use]
pub fn build_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    rule_builder: &dyn Fn(&str, &str, &str) -> serde_json::Value,
) -> serde_json::Value {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();
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
                severity_to_sarif_level(rules.unresolved_imports),
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
        sarif_unused_file_fields(
            &finding.file,
            root,
            severity_to_sarif_level(rules.unused_files),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unused_exports,
        snippets,
        |finding| {
            sarif_export_fields(
                &finding.export,
                root,
                "fallow/unused-export",
                severity_to_sarif_level(rules.unused_exports),
                "Export",
                "Re-export",
            )
        },
    );
    push_sarif_results(sarif_results, &results.unused_types, snippets, |finding| {
        sarif_export_fields(
            &finding.export,
            root,
            "fallow/unused-type",
            severity_to_sarif_level(rules.unused_types),
            "Type export",
            "Type re-export",
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
                severity_to_sarif_level(rules.private_type_leaks),
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

    push_sarif_results(sarif_results, &results.unused_dependencies, snippets, |d| {
        sarif_dep_fields(
            &d.dep,
            root,
            "fallow/unused-dependency",
            severity_to_sarif_level(rules.unused_dependencies),
            "dependencies",
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unused_dev_dependencies,
        snippets,
        |d| {
            sarif_dep_fields(
                &d.dep,
                root,
                "fallow/unused-dev-dependency",
                severity_to_sarif_level(rules.unused_dev_dependencies),
                "devDependencies",
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_optional_dependencies,
        snippets,
        |d| {
            sarif_dep_fields(
                &d.dep,
                root,
                "fallow/unused-optional-dependency",
                severity_to_sarif_level(rules.unused_optional_dependencies),
                "optionalDependencies",
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

    push_sarif_results(
        sarif_results,
        &results.type_only_dependencies,
        snippets,
        |d| {
            sarif_type_only_dep_fields(
                &d.dep,
                root,
                severity_to_sarif_level(rules.type_only_dependencies),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.test_only_dependencies,
        snippets,
        |d| {
            sarif_test_only_dep_fields(
                &d.dep,
                root,
                severity_to_sarif_level(rules.test_only_dependencies),
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
        sarif_member_fields(
            &m.member,
            root,
            "fallow/unused-enum-member",
            severity_to_sarif_level(rules.unused_enum_members),
            "Enum",
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unused_class_members,
        snippets,
        |m| {
            sarif_member_fields(
                &m.member,
                root,
                "fallow/unused-class-member",
                severity_to_sarif_level(rules.unused_class_members),
                "Class",
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_store_members,
        snippets,
        |m| {
            sarif_member_fields(
                &m.member,
                root,
                "fallow/unused-store-member",
                severity_to_sarif_level(rules.unused_store_members),
                "Store",
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
            severity_to_sarif_level(rules.unlisted_dependencies),
            snippets,
        );
    }
    if !results.duplicate_exports.is_empty() {
        push_sarif_duplicate_exports(
            sarif_results,
            &results.duplicate_exports,
            root,
            severity_to_sarif_level(rules.duplicate_exports),
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
                severity_to_sarif_level(rules.unused_component_props),
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
                severity_to_sarif_level(rules.unused_component_emits),
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
                severity_to_sarif_level(rules.unused_component_inputs),
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
                severity_to_sarif_level(rules.unused_component_outputs),
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
                severity_to_sarif_level(rules.unused_svelte_events),
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
                severity_to_sarif_level(rules.unused_server_actions),
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
                severity_to_sarif_level(rules.unused_load_data_keys),
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
            sarif_prop_drilling_fields(&c.chain, root, severity_to_sarif_level(rules.prop_drilling))
        },
    );
    push_sarif_results(sarif_results, &results.thin_wrappers, snippets, |w| {
        sarif_thin_wrapper_fields(
            &w.wrapper,
            root,
            severity_to_sarif_level(rules.thin_wrapper),
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
                severity_to_sarif_level(rules.duplicate_prop_shape),
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
                severity_to_sarif_level(rules.circular_dependencies),
            )
        },
    );
    push_sarif_results(sarif_results, &results.re_export_cycles, snippets, |c| {
        sarif_re_export_cycle_fields(
            &c.cycle,
            root,
            severity_to_sarif_level(rules.re_export_cycle),
        )
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
            severity_to_sarif_level(rules.boundary_violation),
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
                severity_to_sarif_level(rules.boundary_violation),
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
                severity_to_sarif_level(rules.boundary_violation),
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
                severity_to_sarif_level(rules.invalid_client_export),
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
                severity_to_sarif_level(rules.mixed_client_server_barrel),
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
                severity_to_sarif_level(rules.misplaced_directive),
            )
        },
    );
    push_sarif_results(sarif_results, &results.unprovided_injects, snippets, |i| {
        sarif_unprovided_inject_fields(
            &i.inject,
            root,
            severity_to_sarif_level(rules.unprovided_injects),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unrendered_components,
        snippets,
        |c| {
            sarif_unrendered_component_fields(
                &c.component,
                root,
                severity_to_sarif_level(rules.unrendered_components),
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
        sarif_route_collision_fields(
            &c.collision,
            root,
            severity_to_sarif_level(rules.route_collision),
        )
    });
    push_sarif_results(
        sarif_results,
        &results.dynamic_segment_name_conflicts,
        snippets,
        |c| {
            sarif_dynamic_segment_name_conflict_fields(
                &c.conflict,
                root,
                severity_to_sarif_level(rules.dynamic_segment_name_conflict),
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
            severity_to_sarif_level(stale_suppression_severity(s, rules)),
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
                severity_to_sarif_level(rules.unused_catalog_entries),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.empty_catalog_groups,
        snippets,
        |g| {
            sarif_empty_catalog_group_fields(
                g,
                root,
                severity_to_sarif_level(rules.empty_catalog_groups),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unresolved_catalog_references,
        snippets,
        |f| {
            sarif_unresolved_catalog_reference_fields(
                f,
                root,
                severity_to_sarif_level(rules.unresolved_catalog_references),
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
                severity_to_sarif_level(rules.unused_dependency_overrides),
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
                severity_to_sarif_level(rules.misconfigured_dependency_overrides),
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

    #[test]
    fn sarif_rule_list_is_backed_by_issue_contracts() {
        let sarif = build_sarif(
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

        let ids = rules
            .iter()
            .filter_map(|rule| rule.get("id").and_then(serde_json::Value::as_str))
            .collect::<BTreeSet<_>>();

        assert!(ids.contains("fallow/unused-file"));
        assert!(ids.contains("fallow/missing-suppression-reason"));
    }
}
