//! `--format github-annotations`: GitHub Actions workflow-command
//! annotations (`::error` / `::warning` / `::notice` lines on stdout).
//!
//! The per-kind titles and message templates are ported from the bundled
//! action's jq renderers (`action/jq/annotations-{check,dupes,health}.jq`);
//! the security emitter is net-new (the jq layer has no security
//! annotations). Messages are built with real newlines and escaped at the
//! render boundary per the strict contract in [`super::github`].
//!
//! The renderer is value-driven: it consumes the same JSON envelope that
//! `--format json` serializes, which is what makes `fallow report --from
//! <results.json>` byte-identical to the direct format run.

use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

use super::github::{
    Annotation, AnnotationLevel, PackageManager, RenderOptions, arr, b, budget_notice, fmt_num,
    num, one_based_col, render_annotation, resolve_render_options, s, sort_annotations, u,
};
use crate::report::sink::outln;

/// Which JSON envelope family is being rendered. Mirrors the `kind` field on
/// the `--format json` root plus the two aggregate shapes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvelopeKind {
    DeadCode,
    Dupes,
    Health,
    Audit,
    Combined,
    Security,
    /// The `fallow fix --format json` envelope. It carries no top-level `kind`
    /// field (see `crates/output/src/fix.rs`), so `fallow report --from`
    /// detects it by its stable top-level keys rather than a `kind` string.
    Fix,
}

/// Render and print the annotation stream for one envelope, resolving the
/// ambient path-rebase and package-manager options at this boundary.
pub fn print_annotations(kind: EnvelopeKind, envelope: &Value, root: &Path) -> ExitCode {
    let options = resolve_render_options(root);
    let rendered = render_annotations(kind, envelope, &options);
    if !rendered.is_empty() {
        outln!("{rendered}");
    }
    ExitCode::SUCCESS
}

/// Pure renderer: collect per-kind annotations, sort most-severe-first
/// (severity, then path, then line), rebase paths onto the repo root, and
/// append the trailing budget notice.
#[must_use]
pub fn render_annotations(kind: EnvelopeKind, envelope: &Value, options: &RenderOptions) -> String {
    let mut annotations = collect_annotations(kind, envelope, options.pm);
    sort_annotations(&mut annotations);
    let mut lines: Vec<String> = Vec::with_capacity(annotations.len() + 1);
    for annotation in &mut annotations {
        annotation.path = options.rebase.apply(&annotation.path);
        lines.push(render_annotation(annotation));
    }
    if let Some(notice) = budget_notice(annotations.len()) {
        lines.push(notice);
    }
    lines.join("\n")
}

fn collect_annotations(
    kind: EnvelopeKind,
    envelope: &Value,
    pm: PackageManager,
) -> Vec<Annotation> {
    let mut out = Vec::new();
    match kind {
        EnvelopeKind::DeadCode => collect_check(envelope, pm, &mut out),
        EnvelopeKind::Dupes => collect_dupes(envelope, &mut out),
        EnvelopeKind::Health => collect_health(envelope, &mut out),
        EnvelopeKind::Security => collect_security(envelope, &mut out),
        EnvelopeKind::Audit => {
            collect_section(envelope, "dead_code", pm, &mut out, collect_check);
            collect_value_section(envelope, "complexity", &mut out, collect_health);
            collect_value_section(envelope, "duplication", &mut out, collect_dupes);
        }
        EnvelopeKind::Combined => {
            collect_section(envelope, "check", pm, &mut out, collect_check);
            collect_value_section(envelope, "health", &mut out, collect_health);
            collect_value_section(envelope, "dupes", &mut out, collect_dupes);
        }
        // The bundled action no-ops annotations for the fix command
        // (`action/scripts/annotate.sh` skips fix), so the native renderer
        // matches by emitting nothing.
        EnvelopeKind::Fix => {}
    }
    out
}

fn collect_section(
    envelope: &Value,
    key: &str,
    pm: PackageManager,
    out: &mut Vec<Annotation>,
    collect: fn(&Value, PackageManager, &mut Vec<Annotation>),
) {
    if let Some(section) = envelope.get(key).filter(|section| !section.is_null()) {
        collect(section, pm, out);
    }
}

fn collect_value_section(
    envelope: &Value,
    key: &str,
    out: &mut Vec<Annotation>,
    collect: fn(&Value, &mut Vec<Annotation>),
) {
    if let Some(section) = envelope.get(key).filter(|section| !section.is_null()) {
        collect(section, out);
    }
}

/// Line/column anchor for one annotation.
#[derive(Clone, Copy, Default)]
struct Anchor {
    line: Option<u64>,
    col: Option<u64>,
}

impl Anchor {
    /// Unconditional `line=` + 1-based `col=` (the common jq shape).
    fn line_col(item: &Value) -> Self {
        Self {
            line: Some(u(item, "line")),
            col: Some(one_based_col(u(item, "col"))),
        }
    }

    /// `line=` only, no column (jq templates without `col`).
    fn line_only(item: &Value) -> Self {
        Self {
            line: Some(u(item, "line")),
            col: None,
        }
    }

    /// jq's `if .line > 0 then ",line=..,col=.." else ""` gate.
    fn gated_line_col(item: &Value) -> Self {
        if u(item, "line") > 0 {
            Self::line_col(item)
        } else {
            Self::default()
        }
    }

    /// jq's `if .line > 0 then ",line=.." else ""` gate (dependency kinds).
    fn gated_line(item: &Value) -> Self {
        let line = u(item, "line");
        Self {
            line: (line > 0).then_some(line),
            col: None,
        }
    }
}

fn push(
    out: &mut Vec<Annotation>,
    level: AnnotationLevel,
    path: &str,
    anchor: Anchor,
    title: String,
    message: String,
) {
    out.push(Annotation {
        level,
        path: path.to_owned(),
        line: anchor.line,
        end_line: None,
        col: anchor.col,
        title,
        message,
    });
}

/// Emit one warning per item of `env[key]`, with the anchor style chosen by
/// `anchor` and the message built by `message`.
fn push_each(
    out: &mut Vec<Annotation>,
    env: &Value,
    key: &str,
    title: &str,
    anchor: fn(&Value) -> Anchor,
    message: impl Fn(&Value) -> String,
) {
    for item in arr(env, key) {
        push(
            out,
            AnnotationLevel::Warning,
            s(item, "path"),
            anchor(item),
            title.to_owned(),
            message(item),
        );
    }
}

fn no_anchor(_item: &Value) -> Anchor {
    Anchor::default()
}

fn joined_strs(item: &Value, key: &str, separator: &str) -> String {
    arr(item, key)
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(separator)
}

fn workspace_context(item: &Value) -> String {
    let workspaces = joined_strs(item, "used_in_workspaces", ", ");
    if workspaces.is_empty() {
        String::new()
    } else {
        format!("\n\nImported in other workspaces: {workspaces}")
    }
}

fn dependency_action(item: &Value, pm: PackageManager) -> String {
    if arr(item, "used_in_workspaces").next().is_some() {
        "Move this dependency to the consuming workspace package.json.".to_owned()
    } else {
        format!("Run: {}", pm.remove_command(s(item, "package_name")))
    }
}

fn unused_dependency_message(item: &Value, section: &str, pm: PackageManager) -> String {
    format!(
        "Package '{}' is listed in {section} but never imported by this package.{}\n\n{}",
        s(item, "package_name"),
        workspace_context(item),
        dependency_action(item, pm),
    )
}

fn collect_check(env: &Value, pm: PackageManager, out: &mut Vec<Annotation>) {
    collect_check_files_and_exports(env, out);
    collect_check_dependencies(env, pm, out);
    collect_check_members(env, out);
    collect_check_graph(env, out);
    collect_check_boundaries(env, out);
    collect_check_frameworks(env, out);
    collect_check_components(env, out);
    collect_check_suppressions(env, out);
    collect_check_catalog(env, out);
}

fn collect_check_files_and_exports(env: &Value, out: &mut Vec<Annotation>) {
    push_each(out, env, "unused_files", "Unused file", no_anchor, |_| {
        "This file is not imported by any other module and unreachable from entry points.\nConsider removing it or importing it where needed.".to_owned()
    });
    push_each(
        out,
        env,
        "unused_exports",
        "Unused export",
        Anchor::line_col,
        |it| {
            format!(
                "{} {} '{}' is never imported by other modules.\n\nIf this export is part of a public API, consider adding it to the entry configuration.\nOtherwise, remove the export keyword or delete the declaration.",
                if b(it, "is_re_export") {
                    "Re-exported"
                } else {
                    "Exported"
                },
                if b(it, "is_type_only") {
                    "type"
                } else {
                    "value"
                },
                s(it, "export_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_types",
        "Unused type",
        Anchor::line_col,
        |it| {
            format!(
                "{} type '{}' is never imported by other modules.\n\nIf only used internally, remove the export keyword.",
                if b(it, "is_re_export") {
                    "Re-exported"
                } else {
                    "Exported"
                },
                s(it, "export_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "private_type_leaks",
        "Private type leak",
        Anchor::line_col,
        |it| {
            format!(
                "Export '{}' references private type '{}'.\n\nExport the referenced type or remove it from the public signature.",
                s(it, "export_name"),
                s(it, "type_name"),
            )
        },
    );
}

fn collect_check_dependencies(env: &Value, pm: PackageManager, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unused_dependencies",
        "Unused dependency",
        Anchor::gated_line,
        |it| unused_dependency_message(it, "dependencies", pm),
    );
    push_each(
        out,
        env,
        "unused_dev_dependencies",
        "Unused devDependency",
        Anchor::gated_line,
        |it| unused_dependency_message(it, "devDependencies", pm),
    );
    push_each(
        out,
        env,
        "unused_optional_dependencies",
        "Unused optionalDependency",
        Anchor::gated_line,
        |it| unused_dependency_message(it, "optionalDependencies", pm),
    );
    for dependency in arr(env, "unlisted_dependencies") {
        let package = s(dependency, "package_name");
        for site in arr(dependency, "imported_from") {
            push(
                out,
                AnnotationLevel::Warning,
                s(site, "path"),
                Anchor::line_col(site),
                "Unlisted dependency".to_owned(),
                format!(
                    "Package '{package}' is imported here but not listed in package.json.\n\nRun: {}",
                    pm.add_command(package),
                ),
            );
        }
    }
    push_each(
        out,
        env,
        "type_only_dependencies",
        "Type-only dependency",
        Anchor::gated_line,
        |it| {
            format!(
                "Package '{}' is only used via type imports.\n\nMove it from dependencies to devDependencies to reduce production bundle size.",
                s(it, "package_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "test_only_dependencies",
        "Test-only dependency",
        Anchor::gated_line,
        |it| {
            format!(
                "Package '{}' is only imported from test or config files.\n\nMove it from dependencies to devDependencies to reduce production bundle size.",
                s(it, "package_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "dev_dependencies_in_production",
        "Dev dependency in production",
        Anchor::gated_line,
        |it| {
            format!(
                "Package '{}' is a devDependency imported by production code at runtime.\n\nMove it from devDependencies to dependencies so a production-only install does not break at runtime.",
                s(it, "package_name"),
            )
        },
    );
}

fn collect_check_members(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unused_enum_members",
        "Unused enum member",
        Anchor::line_col,
        |it| {
            format!(
                "Enum member '{}.{}' is never referenced in the codebase.\n\nConsider removing it to keep the enum minimal.",
                s(it, "parent_name"),
                s(it, "member_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_class_members",
        "Unused class member",
        Anchor::line_col,
        |it| {
            format!(
                "Class member '{}.{}' is never referenced.\n\nConsider removing it or marking it as private.",
                s(it, "parent_name"),
                s(it, "member_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_store_members",
        "Unused store member",
        Anchor::line_col,
        |it| {
            format!(
                "Store member '{}.{}' is never accessed by any consumer.\n\nConsider removing the unused store state, getter, or action.",
                s(it, "parent_name"),
                s(it, "member_name"),
            )
        },
    );
}

fn collect_check_graph(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unresolved_imports",
        "Unresolved import",
        Anchor::line_col,
        |it| {
            format!(
                "Import '{}' could not be resolved to a file or package.\n\nCheck for typos, missing dependencies, or incorrect path aliases.",
                s(it, "specifier"),
            )
        },
    );
    for duplicate in arr(env, "duplicate_exports") {
        let name = s(duplicate, "export_name");
        let locations: Vec<&Value> = arr(duplicate, "locations").collect();
        let listing = locations
            .iter()
            .map(|location| {
                format!(
                    "  \u{2022} {}:{}",
                    s(location, "path"),
                    num(location, "line")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        for location in &locations {
            push(
                out,
                AnnotationLevel::Warning,
                s(location, "path"),
                Anchor::line_col(location),
                "Duplicate export".to_owned(),
                format!(
                    "Export '{name}' is defined in {} modules:\n{listing}\n\nThis causes ambiguity for consumers. Keep one canonical location.",
                    locations.len(),
                ),
            );
        }
    }
    for cycle in arr(env, "circular_dependencies") {
        let files: Vec<&str> = arr(cycle, "files").filter_map(Value::as_str).collect();
        let first = files.first().copied().unwrap_or_default();
        push(
            out,
            AnnotationLevel::Warning,
            first,
            Anchor::gated_line_col(cycle),
            "Circular dependency".to_owned(),
            format!(
                "Circular import chain detected:\n{} \u{2192} {first}\n\nCircular dependencies can cause initialization bugs and make code harder to reason about.\nConsider extracting shared logic into a separate module.",
                files.join(" \u{2192} "),
            ),
        );
    }
    for cycle in arr(env, "re_export_cycles") {
        let files: Vec<&str> = arr(cycle, "files").filter_map(Value::as_str).collect();
        let kind = s(cycle, "kind");
        let headline = if kind == "self-loop" {
            "Self-loop: this file re-exports from itself.".to_owned()
        } else {
            format!(
                "Re-export cycle ({} files): {}.",
                files.len(),
                files.join(" <-> "),
            )
        };
        let remedy = if kind == "self-loop" {
            "Remove the `export * from './'` (or equivalent) inside this file."
        } else {
            "Remove one `export * from` statement on any one member file to break the cycle."
        };
        push(
            out,
            AnnotationLevel::Warning,
            files.first().copied().unwrap_or_default(),
            Anchor::default(),
            "Re-export cycle".to_owned(),
            format!(
                "{headline}\n\nChain propagation through the loop is a no-op, so imports through any member may silently come up empty.\n{remedy}",
            ),
        );
    }
}

fn collect_check_boundaries(env: &Value, out: &mut Vec<Annotation>) {
    for violation in arr(env, "boundary_violations") {
        push(
            out,
            AnnotationLevel::Warning,
            s(violation, "from_path"),
            Anchor::gated_line_col(violation),
            "Boundary violation".to_owned(),
            format!(
                "Import from zone '{}' to zone '{}' is not allowed.\n{} -> {}\n\nRoute the import through an allowed zone or restructure the dependency.",
                s(violation, "from_zone"),
                s(violation, "to_zone"),
                s(violation, "from_path"),
                s(violation, "to_path"),
            ),
        );
    }
    push_each(
        out,
        env,
        "boundary_coverage_violations",
        "Boundary coverage",
        Anchor::gated_line_col,
        |_| {
            "File does not match any configured architecture boundary zone.\n\nAdd the file to a zone pattern or allow-list it with boundaries.coverage.allowUnmatched.".to_owned()
        },
    );
    push_each(
        out,
        env,
        "boundary_call_violations",
        "Boundary call violation",
        Anchor::gated_line_col,
        |it| {
            format!(
                "Call to '{}' matches forbidden pattern '{}' in zone '{}'.\n\nMove the call behind an allowed abstraction or adjust boundaries.calls.forbidden.",
                s(it, "callee"),
                s(it, "pattern"),
                s(it, "zone"),
            )
        },
    );
    for violation in arr(env, "policy_violations") {
        let level = if s(violation, "severity") == "error" {
            AnnotationLevel::Error
        } else {
            AnnotationLevel::Warning
        };
        let message_suffix = violation
            .get("message")
            .and_then(Value::as_str)
            .map(|message| format!("\n\n{message}"))
            .unwrap_or_default();
        push(
            out,
            level,
            s(violation, "path"),
            Anchor::gated_line_col(violation),
            "Policy violation".to_owned(),
            format!(
                "'{}' is banned by rule '{}/{}'.{message_suffix}",
                s(violation, "matched"),
                s(violation, "pack"),
                s(violation, "rule_id"),
            ),
        );
    }
}

fn collect_check_frameworks(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "invalid_client_exports",
        "Invalid client export",
        Anchor::line_col,
        |it| {
            format!(
                "Export '{}' is not allowed in a \"{directive}\" file (Next.js server-only / route-config name).\n\nMove the server-only export to a non-client module, or remove the \"{directive}\" directive.",
                s(it, "export_name"),
                directive = s(it, "directive"),
            )
        },
    );
    push_each(
        out,
        env,
        "mixed_client_server_barrels",
        "Mixed client/server barrel",
        Anchor::line_col,
        |it| {
            format!(
                "This barrel re-exports both a \"use client\" module ('{}') and a server-only module ('{}'); one import drags the other's directive across the boundary.\n\nSplit the barrel so client and server-only modules are re-exported from separate entry points.",
                s(it, "client_origin"),
                s(it, "server_origin"),
            )
        },
    );
    push_each(
        out,
        env,
        "misplaced_directives",
        "Misplaced directive",
        Anchor::line_col,
        |it| {
            format!(
                "Directive \"{}\" is not in the leading position, so the RSC bundler ignores it.\n\nMove the directive to the very top of the file, above every import.",
                s(it, "directive"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_server_actions",
        "Unused server action",
        Anchor::line_col,
        |it| {
            format!(
                "Server Action '{}' in this \"use server\" file is referenced by no project code.\n\nThe action stays POST-able, but nothing calls it. Remove it to shrink the action surface, or wire it up to a consumer.",
                s(it, "action_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "route_collisions",
        "Route collision",
        no_anchor,
        |it| {
            format!(
                "This route file resolves to '{}', also owned by {} other file(s). Next.js fails the build because a URL can have only one owner.\n\nMove or merge one of the colliding files; route groups and parallel slots do not change the URL.",
                s(it, "url"),
                arr(it, "conflicting_paths").count(),
            )
        },
    );
    push_each(
        out,
        env,
        "dynamic_segment_name_conflicts",
        "Dynamic segment conflict",
        no_anchor,
        |it| {
            format!(
                "Dynamic segments at '{}' use different slug names ({}). Next.js requires one consistent name per dynamic path.\n\nRename the dynamic segments at this position to a single slug name.",
                s(it, "position"),
                joined_strs(it, "conflicting_segments", ", "),
            )
        },
    );
}

fn collect_check_components(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unrendered_components",
        "Unrendered component",
        Anchor::line_col,
        |it| {
            format!(
                "{} component '{}' is reachable but rendered nowhere: no tag, no dynamic binding, no registration.\n\nRender it where it is needed, or remove the component and the re-export keeping it reachable.",
                s(it, "framework"),
                s(it, "component_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_component_props",
        "Unused component prop",
        Anchor::line_col,
        |it| {
            format!(
                "Prop '{}' on component '{}' is referenced nowhere in its own component (neither script nor template).\n\nRemove the prop, or use it. If it is part of a deliberately-stable public API, suppress this finding.",
                s(it, "prop_name"),
                s(it, "component_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_component_emits",
        "Unused component emit",
        Anchor::line_col,
        |it| {
            format!(
                "Emit '{}' on component '{}' is emitted nowhere in its own component.\n\nRemove the emit, or emit it. If it is part of a deliberately-stable public API, suppress this finding.",
                s(it, "emit_name"),
                s(it, "component_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_component_inputs",
        "Unused component input",
        Anchor::line_col,
        |it| {
            format!(
                "Input '{}' on component '{}' is read nowhere in its own component (neither class body nor template).\n\nRemove the input, or use it. If it is part of a deliberately-stable public API, suppress this finding.",
                s(it, "input_name"),
                s(it, "component_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_component_outputs",
        "Unused component output",
        Anchor::line_col,
        |it| {
            format!(
                "Output '{}' on component '{}' is emitted nowhere in its own component.\n\nRemove the output, or emit it. If it is part of a deliberately-stable public API, suppress this finding.",
                s(it, "output_name"),
                s(it, "component_name"),
            )
        },
    );
    collect_check_component_wiring(env, out);
}

fn collect_check_component_wiring(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unused_svelte_events",
        "Unused Svelte event",
        Anchor::line_col,
        |it| {
            format!(
                "Event '{}' dispatched by component '{}' is listened to nowhere in the project.\n\nRemove the dispatched event, or listen for it. If it is part of a deliberately-stable public API, suppress this finding.",
                s(it, "event_name"),
                s(it, "component_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unprovided_injects",
        "Unprovided inject",
        Anchor::line_col,
        |it| {
            format!(
                "{} inject for key '{}' has no matching provider in the project.\n\nAdd a provide/setContext for this key, or remove the dead inject.",
                s(it, "framework"),
                s(it, "key_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_load_data_keys",
        "Unused load data key",
        Anchor::line_only,
        |it| {
            format!(
                "SvelteKit load() return key '{}' is read by no consumer (neither the sibling +page.svelte nor $page.data).\n\nThe key runs a real server fetch / DB cost per request for data nothing renders. Remove the key, or use it.",
                s(it, "key_name"),
            )
        },
    );
}

fn stale_suppression_message(item: &Value) -> (String, String) {
    let origin = item.get("origin").cloned().unwrap_or(Value::Null);
    let comment_form = if b(&origin, "is_file_level") {
        "fallow-ignore-file"
    } else {
        "fallow-ignore-next-line"
    };
    if s(&origin, "type") == "jsdoc_tag" {
        return (
            "Stale @expected-unused".to_owned(),
            format!(
                "The @expected-unused tag on '{}' is stale because the export is now used.\n\nRemove the @expected-unused tag.",
                s(&origin, "export_name"),
            ),
        );
    }
    if origin.get("kind_known").and_then(Value::as_bool) == Some(false) {
        return (
            "Unknown suppression kind".to_owned(),
            format!(
                "'{}' is not a recognized fallow issue kind. Other tokens on this '{comment_form}' line still apply.\n\nFix the typo or remove the unknown token.",
                s(&origin, "issue_kind"),
            ),
        );
    }
    let kind_clause = origin
        .get("issue_kind")
        .and_then(Value::as_str)
        .map(|kind| format!(" for '{kind}'"))
        .unwrap_or_default();
    (
        "Stale suppression".to_owned(),
        format!(
            "This '{comment_form}' comment{kind_clause} no longer matches any active issue.\n\nRemove the suppression comment to keep the codebase clean.",
        ),
    )
}

fn collect_check_suppressions(env: &Value, out: &mut Vec<Annotation>) {
    for item in arr(env, "stale_suppressions") {
        let (title, message) = stale_suppression_message(item);
        push(
            out,
            AnnotationLevel::Warning,
            s(item, "path"),
            Anchor::line_col(item),
            title,
            message,
        );
    }
}

fn unresolved_catalog_reference_message(item: &Value) -> String {
    let catalog = s(item, "catalog_name");
    let (reference, described) = if catalog == "default" {
        (String::new(), "the default catalog".to_owned())
    } else {
        (catalog.to_owned(), format!("catalog '{catalog}'"))
    };
    let available = joined_strs(item, "available_in_catalogs", ", ");
    let remedy = if available.is_empty() {
        "Add this package to the named catalog in pnpm-workspace.yaml, or remove the reference and pin a hardcoded version.".to_owned()
    } else {
        format!(
            "Available in: {available}.\nSwitch the reference to a catalog that declares this package, or add it to the named catalog.",
        )
    };
    format!(
        "Package '{}' is referenced via `catalog:{reference}` but {described} does not declare it. `pnpm install` will fail.\n\n{remedy}",
        s(item, "entry_name"),
    )
}

fn collect_check_catalog(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unused_catalog_entries",
        "Unused catalog entry",
        Anchor::line_only,
        |it| {
            let consumers = joined_strs(it, "hardcoded_consumers", ", ");
            let remedy = if consumers.is_empty() {
                "Remove the entry from pnpm-workspace.yaml.".to_owned()
            } else {
                format!(
                    "Hardcoded consumers: {consumers}.\nSwitch them to catalog: before removing."
                )
            };
            format!(
                "Catalog entry '{}' (catalog '{}') is not referenced by any workspace package via the catalog: protocol.\n\n{remedy}",
                s(it, "entry_name"),
                s(it, "catalog_name"),
            )
        },
    );
    push_each(
        out,
        env,
        "empty_catalog_groups",
        "Empty catalog group",
        Anchor::line_only,
        |it| {
            format!(
                "Catalog group '{}' has no entries.\n\nRemove the empty group header from pnpm-workspace.yaml.",
                s(it, "catalog_name"),
            )
        },
    );
    for item in arr(env, "unresolved_catalog_references") {
        push(
            out,
            AnnotationLevel::Error,
            s(item, "path"),
            Anchor::line_only(item),
            "Unresolved catalog reference".to_owned(),
            unresolved_catalog_reference_message(item),
        );
    }
    push_each(
        out,
        env,
        "unused_dependency_overrides",
        "Unused dependency override",
        Anchor::line_only,
        |it| {
            let target = s(it, "target_package");
            let hint = it
                .get("hint")
                .and_then(Value::as_str)
                .map(|hint| format!("{hint}.\n"))
                .unwrap_or_default();
            format!(
                "Override `{}` forces `{target}` to `{}` but no workspace package depends on `{target}`.\n\n{hint}Delete the entry, or scope it under a real parent (`pkg>{target}`) if it pins a transitive.",
                s(it, "raw_key"),
                s(it, "version_range"),
            )
        },
    );
    for item in arr(env, "misconfigured_dependency_overrides") {
        let reason = item
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unparsable");
        push(
            out,
            AnnotationLevel::Error,
            s(item, "path"),
            Anchor::line_only(item),
            "Misconfigured dependency override".to_owned(),
            format!(
                "Override `{}` -> `{}` is malformed ({reason}). `pnpm install` will reject this entry.\n\nFix the key/value to match pnpm's override grammar, or remove the entry.",
                s(item, "raw_key"),
                s(item, "raw_value"),
            ),
        );
    }
}

fn short_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 3 {
        segments[segments.len() - 3..].join("/")
    } else {
        segments.join("/")
    }
}

fn collect_dupes(env: &Value, out: &mut Vec<Annotation>) {
    for group in arr(env, "clone_groups") {
        let instances: Vec<&Value> = arr(group, "instances").collect();
        for instance in &instances {
            // jq removes every instance deep-equal to the current one, so
            // identical duplicates drop out of their own "Also in" list.
            let others = instances
                .iter()
                .filter(|other| ***other != **instance)
                .fold(String::new(), |mut acc, other| {
                    use std::fmt::Write as _;
                    let _ = write!(
                        acc,
                        "\n  \u{2192} {}:{}-{}",
                        short_path(s(other, "file")),
                        num(other, "start_line"),
                        num(other, "end_line"),
                    );
                    acc
                });
            out.push(Annotation {
                level: AnnotationLevel::Warning,
                path: s(instance, "file").to_owned(),
                line: Some(u(instance, "start_line")),
                end_line: Some(u(instance, "end_line")),
                col: Some(one_based_col(u(instance, "start_col"))),
                title: "Code duplication".to_owned(),
                message: format!(
                    "{} duplicated lines ({} tokens)\n\n{} instances found. Also in:{others}\n\nExtract a shared function to eliminate this duplication.",
                    num(group, "line_count"),
                    num(group, "token_count"),
                    instances.len(),
                ),
            });
        }
    }
}

fn threshold(env: &Value, key: &str, default: &str) -> String {
    env.get("summary")
        .and_then(|summary| summary.get(key))
        .filter(|value| !value.is_null())
        .map_or_else(|| default.to_owned(), fmt_num)
}

/// Health complexity severity to workflow-command level: `critical` and
/// `high` map to `::error` (consistent with SARIF's `error` for critical;
/// panel decision), everything else to `::warning`.
fn complexity_level(severity: &str) -> AnnotationLevel {
    if matches!(severity, "critical" | "high") {
        AnnotationLevel::Error
    } else {
        AnnotationLevel::Warning
    }
}

struct ComplexityThresholds {
    cyclomatic: String,
    cognitive: String,
    crap: String,
}

fn complexity_annotation(finding: &Value, ctx: &ComplexityThresholds) -> (String, String) {
    let severity = finding
        .get("severity")
        .and_then(Value::as_str)
        .unwrap_or("moderate");
    let name = s(finding, "name");
    let cyclomatic = num(finding, "cyclomatic");
    let cognitive = num(finding, "cognitive");
    let lines = num(finding, "line_count");
    let crap_line = finding
        .get("crap")
        .filter(|crap| !crap.is_null())
        .map(|crap| {
            format!(
                "  \u{2022} CRAP: {} (threshold: {})\n",
                fmt_num(crap),
                ctx.crap
            )
        })
        .unwrap_or_default();
    match s(finding, "exceeded") {
        "crap" | "cyclomatic_crap" | "cognitive_crap" | "all" => (
            format!("High CRAP score ({severity})"),
            format!(
                "Function '{name}' has a CRAP score of {} (threshold: {}).\n\n  \u{2022} Severity: {severity}\n  \u{2022} Cyclomatic: {cyclomatic}\n  \u{2022} Cognitive: {cognitive}\n{crap_line}  \u{2022} Lines: {lines}\n\nCRAP combines complexity with coverage: high CRAP means changes here carry high risk.\nConsider adding tests, simplifying the function, or both.",
                num(finding, "crap"),
                ctx.crap,
            ),
        ),
        "both" => (
            format!("High complexity ({severity})"),
            format!(
                "Function '{name}' exceeds both complexity thresholds:\n\n  \u{2022} Severity: {severity}\n  \u{2022} Cyclomatic: {cyclomatic} (threshold: {})\n  \u{2022} Cognitive: {cognitive} (threshold: {})\n{crap_line}  \u{2022} Lines: {lines}\n\nConsider splitting this function into smaller, focused functions.",
                ctx.cyclomatic, ctx.cognitive,
            ),
        ),
        "cyclomatic" => (
            format!("High cyclomatic complexity ({severity})"),
            format!(
                "Function '{name}' has {cyclomatic} code paths (threshold: {}).\n\n  \u{2022} Severity: {severity}\n  \u{2022} Cyclomatic: {cyclomatic}\n  \u{2022} Cognitive: {cognitive}\n{crap_line}  \u{2022} Lines: {lines}\n\nHigh cyclomatic complexity means many branches to test.\nConsider extracting conditionals or using early returns.",
                ctx.cyclomatic,
            ),
        ),
        _ => (
            format!("High cognitive complexity ({severity})"),
            format!(
                "Function '{name}' is hard to understand (cognitive: {cognitive}, threshold: {}).\n\n  \u{2022} Severity: {severity}\n  \u{2022} Cyclomatic: {cyclomatic}\n  \u{2022} Cognitive: {cognitive}\n{crap_line}  \u{2022} Lines: {lines}\n\nHigh cognitive complexity means deeply nested or interleaved logic.\nConsider flattening control flow or extracting helper functions.",
                ctx.cognitive,
            ),
        ),
    }
}

fn collect_health(env: &Value, out: &mut Vec<Annotation>) {
    let ctx = ComplexityThresholds {
        cyclomatic: threshold(env, "max_cyclomatic_threshold", "20"),
        cognitive: threshold(env, "max_cognitive_threshold", "15"),
        crap: threshold(env, "max_crap_threshold", "30"),
    };
    for finding in arr(env, "findings") {
        let severity = finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("moderate");
        let (title, message) = complexity_annotation(finding, &ctx);
        push(
            out,
            complexity_level(severity),
            s(finding, "path"),
            Anchor::line_col(finding),
            title,
            message,
        );
    }
    collect_runtime_coverage(env, out);
    collect_targets(env, out);
}

fn collect_runtime_coverage(env: &Value, out: &mut Vec<Annotation>) {
    let Some(runtime) = env.get("runtime_coverage") else {
        return;
    };
    for finding in arr(runtime, "findings") {
        let verdict = s(finding, "verdict");
        let level = if verdict == "coverage_unavailable" {
            AnnotationLevel::Notice
        } else {
            AnnotationLevel::Warning
        };
        let invocations = finding
            .get("invocations")
            .filter(|value| !value.is_null())
            .map_or_else(|| "-".to_owned(), fmt_num);
        let evidence = finding.get("evidence").cloned().unwrap_or(Value::Null);
        let tracking = evidence
            .get("untracked_reason")
            .and_then(Value::as_str)
            .map_or_else(
                || s(&evidence, "v8_tracking").to_owned(),
                |reason| format!("{} ({reason})", s(&evidence, "v8_tracking")),
            );
        let advice = arr(finding, "actions")
            .next()
            .and_then(|action| action.get("description"))
            .and_then(Value::as_str)
            .unwrap_or("Review the runtime evidence before changing this path.");
        push(
            out,
            level,
            s(finding, "path"),
            Anchor::line_only(finding),
            format!("Runtime coverage ({verdict})"),
            format!(
                "Function '{}' is flagged by runtime coverage.\n\n  \u{2022} Verdict: {verdict}\n  \u{2022} Invocations: {invocations}\n  \u{2022} Confidence: {}\n  \u{2022} Static: {}\n  \u{2022} Tests: {}\n  \u{2022} V8: {tracking}\n\n{advice}",
                s(finding, "function"),
                s(finding, "confidence"),
                s(&evidence, "static_status"),
                s(&evidence, "test_coverage"),
            ),
        );
    }
}

fn collect_targets(env: &Value, out: &mut Vec<Annotation>) {
    let targets = env
        .get("targets")
        .filter(|value| !value.is_null())
        .or_else(|| env.get("refactoring_targets"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    // The jq template annotates only the top 5 targets (a content decision in
    // `annotations-health.jq`, distinct from the removed MAX_ANNOTATIONS cap).
    for target in targets.iter().take(5) {
        let factors = target
            .get("factors")
            .and_then(Value::as_array)
            .map(|factors| {
                factors
                    .iter()
                    .map(|factor| {
                        let detail = factor
                            .get("detail")
                            .and_then(Value::as_str)
                            .map_or_else(|| num(factor, "value"), str::to_owned);
                        format!("  \u{2022} {}: {detail}", s(factor, "metric"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        push(
            out,
            AnnotationLevel::Notice,
            s(target, "path"),
            Anchor::default(),
            format!("Refactoring target ({} effort)", s(target, "effort")),
            format!(
                "Priority: {} | Confidence: {}\n\n{}\n\n{factors}",
                s(target, "priority"),
                s(target, "confidence"),
                s(target, "recommendation"),
            ),
        );
    }
}

/// Net-new security annotations (the jq layer has no
/// `annotations-security.jq`): every candidate renders at `::notice`, because
/// fallow surfaces unverified candidates, not confirmed vulnerabilities.
fn collect_security(env: &Value, out: &mut Vec<Annotation>) {
    for finding in arr(env, "security_findings") {
        let kind = s(finding, "kind");
        let severity = finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let callee = finding
            .get("candidate")
            .and_then(|candidate| candidate.get("sink"))
            .and_then(|sink| sink.get("callee"))
            .and_then(Value::as_str)
            .filter(|callee| !callee.is_empty())
            .unwrap_or("-");
        push(
            out,
            AnnotationLevel::Notice,
            s(finding, "path"),
            Anchor::line_col(finding),
            format!("Security candidate ({kind})"),
            format!(
                "Local security candidate (severity: {severity}).\n\n  \u{2022} Sink: {callee}\n  \u{2022} Evidence: {}\n\nTreat this as a candidate for verification, not a confirmed vulnerability.",
                s(finding, "evidence"),
            ),
        );
    }
}
