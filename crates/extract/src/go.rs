//! Go 1.25+ source file parser.
//!
//! Extracts package imports and exported declarations using a byte-level scanner.
//! No external Go AST library is required; Go's declaration syntax is regular
//! enough for a hand-rolled scanner to handle all common patterns.
//!
//! # What is extracted
//!
//! **Imports** — all forms of the `import` declaration:
//! - Single: `import "path"`, `import alias "path"`, `import . "path"`, `import _ "path"`
//! - Grouped: `import (\n  "path"\n  alias "path"\n)`
//!
//! **Exports** — top-level declarations whose identifier begins with an uppercase
//! letter (Go's visibility rule). Exported type declarations also collect
//! exported receiver methods and exported struct/interface members.
//!
//! **Always-live** — `init`, `main` (in any `.go` file), and test/bench/example/
//! fuzz functions in `_test.go` files are marked with `VisibilityTag::Public` so
//! the analysis layer never reports them as unused.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

use oxc_span::Span;
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;

use crate::{
    ExportInfo, ExportName, ImportInfo, ImportedName, MemberInfo, MemberKind, ModuleInfo,
    VisibilityTag,
};
use fallow_types::discover::FileId;
use fallow_types::extract::{
    ClassHeritageInfo, FlagUse, FlagUseKind, LocalTypeDeclaration, MemberAccess,
    PublicSignatureTypeReference, byte_offset_to_line_col, compute_line_offsets,
};

// ── Regexes ──────────────────────────────────────────────────────────────────

/// Matches a single-line import: `import [alias] "path"` or `import [alias] 'path'`
/// Groups: (1) optional alias, (2) import path
static SINGLE_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^import\s+(?:(\.|_|[[:alpha:]][[:alnum:]_]*)\s+)?["']([^"']+)["']"#)
        .expect("valid regex")
});

/// Matches the entire body of a grouped import block: `import (\n  ...\n)`
static GROUP_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\bimport\s*\(([^)]*)\)").expect("valid regex"));

/// Matches one entry inside a group: `[alias] "path"`
/// Groups: (1) optional alias, (2) import path
static GROUP_ENTRY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:(\.|_|[[:alpha:]][[:alnum:]_]*)\s+)?["']([^"']+)["']"#)
        .expect("valid regex")
});

// ── Public API ────────────────────────────────────────────────────────────────

pub(crate) fn is_go_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "go")
}

/// Parse a Go source file into a [`ModuleInfo`].
pub(crate) fn parse_go_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let is_test_file = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("_test.go"));

    let stripped = strip_go_comments(source);
    let imports = extract_imports(&stripped);
    let mut exports = extract_exports(&stripped, source, is_test_file);
    let parser_output = run_go_parser_helper(path, source);
    let parser_output_available = parser_output.is_some();
    if let Some(parsed) = parser_output.as_ref() {
        attach_receiver_members_from_parser(&mut exports, &parsed.types);
        ensure_export_local_names(&mut exports);
    } else {
        attach_receiver_members(&mut exports, &stripped, source);
    }
    let class_heritage = parser_output
        .as_ref()
        .map(|parsed| {
            parsed
                .heritage
                .iter()
                .map(|heritage| ClassHeritageInfo {
                    export_name: heritage.export_name.clone(),
                    super_class: None,
                    implements: heritage.implements.clone(),
                    instance_bindings: Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut member_accesses = extract_member_accesses(&stripped, &imports);
    member_accesses.extend(extract_dot_import_accesses(&stripped, &imports, &exports));
    if let Some(parsed) = parser_output {
        member_accesses.extend(
            parsed
                .member_accesses
                .into_iter()
                .map(|access| MemberAccess {
                    object: access.object,
                    member: access.member,
                }),
        );
    }
    member_accesses.extend(extract_type_binding_member_accesses(
        path,
        &stripped,
        &imports,
        &exports,
        parser_output_available,
    ));
    dedupe_member_accesses(&mut member_accesses);
    let local_type_declarations = extract_local_type_declarations(&stripped, source);
    let public_signature_type_references =
        extract_public_signature_type_references(&stripped, &local_type_declarations);
    let line_offsets = compute_line_offsets(source);
    let import_binding_usage = compute_go_import_binding_usage(&imports, &member_accesses);
    let flag_uses = extract_go_flags(&stripped, &imports, &line_offsets, &[], &[]);
    let complexity = if need_complexity {
        extract_complexity(&stripped, &line_offsets)
    } else {
        Vec::new()
    };

    ModuleInfo {
        file_id,
        exports,
        imports,
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        member_accesses,
        whole_object_uses: Vec::new(),
        has_cjs_exports: false,
        content_hash,
        suppressions: crate::suppress::parse_suppressions_from_source(source),
        unused_import_bindings: import_binding_usage.unused,
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: import_binding_usage.value_referenced,
        line_offsets,
        complexity,
        flag_uses,
        class_heritage,
        local_type_declarations,
        public_signature_type_references,
    }
}

#[derive(Debug, Deserialize)]
struct ParserHelperOutput {
    #[serde(default)]
    types: Vec<ParserHelperType>,
    #[serde(default)]
    heritage: Vec<ParserHelperHeritage>,
    #[serde(default)]
    member_accesses: Vec<ParserHelperAccess>,
}

#[derive(Debug, Deserialize)]
struct ParserHelperType {
    name: String,
    members: Vec<ParserHelperMember>,
}

#[derive(Debug, Deserialize)]
struct ParserHelperMember {
    name: String,
    kind: String,
    start: u32,
    end: u32,
}

#[derive(Debug, Deserialize)]
struct ParserHelperHeritage {
    export_name: String,
    #[serde(default)]
    implements: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ParserHelperAccess {
    object: String,
    member: String,
}

static GO_MEMBER_HELPER: LazyLock<Mutex<Option<std::path::PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));
static GO_MEMBER_HELPER_RUN_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn run_go_parser_helper(path: &Path, source: &str) -> Option<ParserHelperOutput> {
    let _run_guard = GO_MEMBER_HELPER_RUN_LOCK.lock().ok()?;
    if let Some(helper_path) = go_member_helper_path()
        && let Some(parsed) = run_go_parser_command(Command::new(&helper_path).arg(path), source)
    {
        return Some(parsed);
    }
    reset_go_member_helper();
    if let Some(helper_path) = go_member_helper_path()
        && let Some(parsed) = run_go_parser_command(Command::new(&helper_path).arg(path), source)
    {
        return Some(parsed);
    }

    let helper_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("go_parser")
        .join("main.go");
    run_go_parser_command(
        Command::new("go")
            .arg("run")
            .arg(&helper_src)
            .arg("--")
            .arg(path),
        source,
    )
}

fn run_go_parser_command(command: &mut Command, source: &str) -> Option<ParserHelperOutput> {
    let Ok(mut child) = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return None;
    };

    if let Some(mut stdin) = child.stdin.take()
        && std::io::Write::write_all(&mut stdin, source.as_bytes()).is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }

    let output = match child.wait_with_output() {
        Ok(output) if output.status.success() => output,
        Ok(_) | Err(_) => return None,
    };
    serde_json::from_slice(&output.stdout).ok()
}

fn attach_receiver_members_from_parser(exports: &mut [ExportInfo], types: &[ParserHelperType]) {
    let mut members_by_type: FxHashMap<String, Vec<MemberInfo>> = FxHashMap::default();
    for type_info in types {
        let members = type_info
            .members
            .iter()
            .map(|member| MemberInfo {
                name: member.name.clone(),
                kind: if member.kind == "property" {
                    MemberKind::ClassProperty
                } else {
                    MemberKind::ClassMethod
                },
                span: Span::new(member.start, member.end),
                has_decorator: false,
            })
            .collect::<Vec<_>>();
        members_by_type.insert(type_info.name.clone(), members);
    }

    for export in exports.iter_mut() {
        if let ExportName::Named(name) = &export.name
            && let Some(members) = members_by_type.remove(name)
        {
            export.members = members;
        }
    }
}

fn ensure_export_local_names(exports: &mut [ExportInfo]) {
    for export in exports {
        if let ExportName::Named(name) = &export.name {
            export.local_name = Some(name.clone());
        }
    }
}

fn go_member_helper_path() -> Option<std::path::PathBuf> {
    let mut helper = GO_MEMBER_HELPER.lock().ok()?;
    if let Some(path) = helper.as_ref() {
        return Some(path.clone());
    }
    let built = build_go_member_helper()?;
    *helper = Some(built.clone());
    drop(helper);
    Some(built)
}

fn reset_go_member_helper() {
    if let Ok(mut helper) = GO_MEMBER_HELPER.lock() {
        *helper = None;
    }
}

fn build_go_member_helper() -> Option<std::path::PathBuf> {
    let helper_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("go_parser")
        .join("main.go");
    let out_path = std::env::temp_dir().join(format!(
        "fallow-go-member-helper-{}-{}",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    ));

    let output = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&out_path)
        .arg(&helper_src)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(out_path)
}

fn dedupe_member_accesses(accesses: &mut Vec<MemberAccess>) {
    accesses.sort_by(|a, b| {
        a.object
            .cmp(&b.object)
            .then_with(|| a.member.cmp(&b.member))
    });
    accesses.dedup_by(|a, b| a.object == b.object && a.member == b.member);
}

// ── Comment stripping ─────────────────────────────────────────────────────────

/// Return source with `//` line comments and `/* */` block comments replaced by
/// spaces. Preserves byte positions so that regex matches on the stripped string
/// correspond to the same offsets in the original.
fn strip_go_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = source.to_string().into_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Skip string literals: "..." and `...` — do not strip inside them.
        if bytes[i] == b'"' {
            i += 1;
            while i < len && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'`' {
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'\'' {
            i += 1;
            while i < len && bytes[i] != b'\'' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        // Line comment: // ...
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
            continue;
        }

        // Block comment: /* ... */
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            if i + 1 < len {
                out[i] = b' ';
                out[i + 1] = b' ';
                i += 2;
            }
            continue;
        }

        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

// ── Import extraction ─────────────────────────────────────────────────────────

fn extract_imports(stripped: &str) -> Vec<ImportInfo> {
    let mut imports: Vec<ImportInfo> = Vec::new();
    // Track which byte ranges were consumed by group imports to avoid double-counting.
    let mut group_ranges: Vec<std::ops::Range<usize>> = Vec::new();

    // Group imports first.
    for cap in GROUP_IMPORT_RE.captures_iter(stripped) {
        let full = cap.get(0).expect("full match");
        let body = cap.get(1).expect("group body");
        group_ranges.push(full.start()..full.end());

        for entry in GROUP_ENTRY_RE.captures_iter(body.as_str()) {
            let alias_str = entry.get(1).map(|m| m.as_str());
            let path_str = entry.get(2).expect("path").as_str();
            let byte_offset = full
                .start()
                .saturating_add(body.start())
                .saturating_add(entry.get(0).expect("entry match").start())
                as u32;
            imports.push(make_import(alias_str, path_str, byte_offset));
        }
    }

    // Single-line imports that are not inside a group block.
    for cap in SINGLE_IMPORT_RE.captures_iter(stripped) {
        let full = cap.get(0).expect("full match");
        if group_ranges.iter().any(|r| r.contains(&full.start())) {
            continue;
        }
        let alias_str = cap.get(1).map(|m| m.as_str());
        let path_str = cap.get(2).expect("path").as_str();
        imports.push(make_import(alias_str, path_str, full.start() as u32));
    }

    imports
}

fn make_import(alias: Option<&str>, path: &str, byte_offset: u32) -> ImportInfo {
    let span = Span::new(byte_offset, byte_offset);

    let (imported_name, local_name) = match alias {
        Some("_") => (ImportedName::SideEffect, String::new()),
        Some(".") => {
            // Dot import: all exported names available without qualifier.
            (ImportedName::Namespace, String::new())
        }
        Some(alias) => {
            // Explicit alias: `import m "math"` — local name is `m`.
            (ImportedName::Namespace, alias.to_string())
        }
        None => {
            // Unaliased: local name is the last path segment (Go convention).
            let local = path.rsplit('/').next().unwrap_or(path).to_string();
            (ImportedName::Namespace, local)
        }
    };

    ImportInfo {
        source: path.to_string(),
        imported_name,
        local_name,
        is_type_only: false,
        from_style: false,
        span,
        source_span: span,
    }
}

fn extract_member_accesses(stripped: &str, imports: &[ImportInfo]) -> Vec<MemberAccess> {
    let package_locals: FxHashSet<&str> = imports
        .iter()
        .filter(|imp| {
            matches!(imp.imported_name, ImportedName::Namespace) && !imp.local_name.is_empty()
        })
        .map(|imp| imp.local_name.as_str())
        .collect();

    if package_locals.is_empty() {
        return Vec::new();
    }

    let bytes = stripped.as_bytes();
    let mut i = 0;
    let mut accesses = Vec::new();

    while i < bytes.len() {
        let start = i;
        let Some(object) = parse_ident(bytes, &mut i) else {
            i = start.saturating_add(1);
            continue;
        };

        if !package_locals.contains(object) || i >= bytes.len() || bytes[i] != b'.' {
            continue;
        }

        i += 1;
        let mut member_i = i;
        let Some(member) = parse_ident(bytes, &mut member_i) else {
            continue;
        };
        i = member_i;

        // Go package-qualified references only expose exported identifiers.
        if is_go_exported(member) {
            accesses.push(MemberAccess {
                object: object.to_string(),
                member: member.to_string(),
            });
            let mut qualified_base_i = skip_ascii_whitespace(bytes, i, bytes.len());
            if qualified_base_i < bytes.len() && bytes[qualified_base_i] == b'[' {
                qualified_base_i = find_matching(bytes, qualified_base_i, bytes.len(), b'[', b']')
                    .unwrap_or(qualified_base_i)
                    .saturating_add(1);
            }
            if qualified_base_i < bytes.len() && bytes[qualified_base_i] == b'.' {
                let mut qualified_i = qualified_base_i + 1;
                if let Some(qualified_member) = parse_ident(bytes, &mut qualified_i)
                    && is_go_exported(qualified_member)
                {
                    accesses.push(MemberAccess {
                        object: format!("{object}.{member}"),
                        member: qualified_member.to_string(),
                    });
                    i = qualified_i;
                }
            }
        }
    }

    accesses
}

fn extract_dot_import_accesses(
    stripped: &str,
    imports: &[ImportInfo],
    exports: &[ExportInfo],
) -> Vec<MemberAccess> {
    let has_dot_import = imports.iter().any(|import| {
        matches!(import.imported_name, ImportedName::Namespace) && import.local_name.is_empty()
    });
    if !has_dot_import {
        return Vec::new();
    }

    let exported_locals: FxHashSet<&str> = exports
        .iter()
        .filter_map(|export| match &export.name {
            ExportName::Named(name) => Some(name.as_str()),
            ExportName::Default => None,
        })
        .collect();

    let bytes = stripped.as_bytes();
    let mut accesses = Vec::new();
    let mut i = 0;
    let mut prev_ident: Option<&str> = None;

    while i < bytes.len() {
        let start = i;
        let Some(ident) = parse_ident(bytes, &mut i) else {
            prev_ident = None;
            i = start.saturating_add(1);
            continue;
        };

        if !is_go_exported(ident)
            || exported_locals.contains(ident)
            || is_declaration_keyword(ident)
            || previous_non_whitespace(bytes, start) == Some(b'.')
            || previous_identifier_introduces_declaration(prev_ident)
            || next_tokens_are_short_declaration(bytes, i)
        {
            prev_ident = Some(ident);
            continue;
        }

        accesses.push(MemberAccess {
            object: String::new(),
            member: ident.to_string(),
        });
        prev_ident = Some(ident);
    }

    accesses.sort_by(|a, b| a.member.cmp(&b.member));
    accesses.dedup_by(|a, b| a.object == b.object && a.member == b.member);
    accesses
}

fn extract_type_binding_member_accesses(
    path: &Path,
    stripped: &str,
    imports: &[ImportInfo],
    exports: &[ExportInfo],
    parser_output_available: bool,
) -> Vec<MemberAccess> {
    let owned_export_types = collect_same_package_export_type_names(path, stripped, exports);
    let export_types: FxHashSet<&str> = owned_export_types
        .iter()
        .map(std::string::String::as_str)
        .collect();
    if export_types.is_empty() && imports.is_empty() {
        return Vec::new();
    }

    let package_locals: FxHashSet<&str> = imports
        .iter()
        .filter(|imp| {
            matches!(imp.imported_name, ImportedName::Namespace) && !imp.local_name.is_empty()
        })
        .map(|imp| imp.local_name.as_str())
        .collect();
    let helper_targets = collect_go_helper_targets(
        path,
        stripped.as_bytes(),
        imports,
        &export_types,
        &package_locals,
    );

    let mut bindings: FxHashMap<String, String> = FxHashMap::default();
    extract_go_type_bindings(
        stripped.as_bytes(),
        &export_types,
        &package_locals,
        &helper_targets,
        &mut bindings,
    );

    let bytes = stripped.as_bytes();
    let mut accesses = collect_bound_member_accesses(bytes, &bindings);
    accesses.extend(extract_direct_target_member_accesses(
        bytes,
        &export_types,
        &package_locals,
        &helper_targets,
    ));
    if !parser_output_available {
        accesses.extend(extract_function_param_binding_accesses(
            bytes,
            &export_types,
            &package_locals,
            &helper_targets,
        ));
    }
    accesses.extend(extract_type_switch_binding_accesses(
        bytes,
        &export_types,
        &package_locals,
        &helper_targets,
    ));
    if accesses.is_empty() {
        return Vec::new();
    }

    accesses.sort_by(|a, b| {
        a.object
            .cmp(&b.object)
            .then_with(|| a.member.cmp(&b.member))
    });
    accesses.dedup_by(|a, b| a.object == b.object && a.member == b.member);
    accesses
}

fn collect_same_package_export_type_names(
    path: &Path,
    stripped: &str,
    exports: &[ExportInfo],
) -> FxHashSet<String> {
    let mut export_types: FxHashSet<String> = exports
        .iter()
        .filter(|export| export.is_type_only)
        .filter_map(|export| match &export.name {
            ExportName::Named(name) => Some(name.clone()),
            ExportName::Default => None,
        })
        .collect();

    let Some(dir) = path.parent() else {
        return export_types;
    };
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return export_types;
    };
    let is_test_file = file_name.ends_with("_test.go");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return export_types;
    };

    for entry in entries.flatten() {
        let sibling_path = entry.path();
        if sibling_path == path {
            continue;
        }
        if sibling_path.extension().and_then(|ext| ext.to_str()) != Some("go") {
            continue;
        }
        let sibling_is_test = sibling_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_test.go"));
        if sibling_is_test != is_test_file {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&sibling_path) else {
            continue;
        };
        let sibling_stripped = strip_go_comments(&source);
        if !same_go_package(stripped, &sibling_stripped) {
            continue;
        }
        let sibling_exports = extract_exports(&sibling_stripped, &source, sibling_is_test);
        export_types.extend(
            sibling_exports
                .into_iter()
                .filter(|export| export.is_type_only)
                .filter_map(|export| match export.name {
                    ExportName::Named(name) => Some(name),
                    ExportName::Default => None,
                }),
        );
    }

    export_types
}

fn same_go_package(left: &str, right: &str) -> bool {
    go_package_name(left)
        .zip(go_package_name(right))
        .is_some_and(|(left_name, right_name)| left_name == right_name)
}

fn go_package_name(source: &str) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let Some(ident) = parse_ident(bytes, &mut i) else {
            i = start.saturating_add(1);
            continue;
        };
        if ident != "package" {
            continue;
        }
        i = skip_ascii_whitespace(bytes, i, bytes.len());
        return parse_ident(bytes, &mut i);
    }
    None
}

fn extract_direct_target_member_accesses(
    bytes: &[u8],
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Vec<MemberAccess> {
    let mut accesses = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some(next) = skip_string_or_rune(bytes, i, bytes.len()) {
            i = next;
            continue;
        }

        let start = i;
        let mut cursor = i;
        let Some(target) = parse_type_target_from_expression(
            bytes,
            &mut cursor,
            export_types,
            package_locals,
            helper_targets,
        ) else {
            i = start.saturating_add(1);
            continue;
        };

        let member_dot = skip_ascii_whitespace(bytes, cursor, bytes.len());
        if member_dot >= bytes.len() || bytes[member_dot] != b'.' {
            i = start.saturating_add(1);
            continue;
        }

        let mut member_i = member_dot + 1;
        let Some(member) = parse_ident(bytes, &mut member_i) else {
            i = start.saturating_add(1);
            continue;
        };
        if is_go_exported(member) {
            accesses.push(MemberAccess {
                object: target,
                member: member.to_string(),
            });
            i = member_i;
            continue;
        }

        i = start.saturating_add(1);
    }

    accesses
}

fn extract_function_param_binding_accesses(
    bytes: &[u8],
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Vec<MemberAccess> {
    let mut accesses = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some(next) = skip_string_or_rune(bytes, i, bytes.len()) {
            i = next;
            continue;
        }
        if let Some(function) = parse_function_at(bytes, i, bytes.len()) {
            let mut bindings = FxHashMap::default();
            collect_function_param_bindings(
                bytes,
                function.params_start.saturating_add(1),
                function.params_end,
                export_types,
                package_locals,
                &mut bindings,
            );
            if !bindings.is_empty() && function.body_start < function.body_end {
                let body = &bytes[function.body_start + 1..function.body_end];
                extract_go_type_bindings(
                    body,
                    export_types,
                    package_locals,
                    helper_targets,
                    &mut bindings,
                );
                accesses.extend(collect_bound_member_accesses(body, &bindings));
            }
            i = function.body_end.saturating_add(1);
            continue;
        }
        i += 1;
    }

    accesses
}

fn collect_function_param_bindings(
    bytes: &[u8],
    start: usize,
    end: usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    bindings: &mut FxHashMap<String, String>,
) {
    let mut i = start;
    while i < end {
        let field_end = skip_go_argument(bytes, i, end);
        let mut cursor = skip_ascii_whitespace(bytes, i, field_end);
        let Some(name) = parse_ident(bytes, &mut cursor) else {
            i = field_end.saturating_add(1);
            continue;
        };
        if let Some(target) = parse_type_target(bytes, &mut cursor, export_types, package_locals) {
            bindings.insert(name.to_string(), target);
        }
        i = field_end.saturating_add(1);
    }
}

fn extract_type_switch_binding_accesses(
    bytes: &[u8],
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Vec<MemberAccess> {
    let mut accesses = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some(next) = skip_string_or_rune(bytes, i, bytes.len()) {
            i = next;
            continue;
        }
        if !is_keyword_at(bytes, i, b"switch") {
            i += 1;
            continue;
        }

        let header_start = i + 6;
        let Some(body_start) = find_statement_block_start(bytes, header_start, bytes.len()) else {
            i += 1;
            continue;
        };
        let Some(body_end) = find_matching(bytes, body_start, bytes.len(), b'{', b'}') else {
            i += 1;
            continue;
        };

        let Some(binding_name) = parse_type_switch_binding_name(bytes, header_start, body_start)
        else {
            i = body_end.saturating_add(1);
            continue;
        };

        let mut clause_start = body_start + 1;
        while clause_start < body_end {
            clause_start = skip_ascii_whitespace(bytes, clause_start, body_end);
            if clause_start >= body_end {
                break;
            }
            let is_case = is_keyword_at(bytes, clause_start, b"case");
            let is_default = is_keyword_at(bytes, clause_start, b"default");
            if !is_case && !is_default {
                clause_start += 1;
                continue;
            }

            let Some(clause_header_end) =
                find_case_clause_header_end(bytes, clause_start, body_end)
            else {
                break;
            };
            let clause_end = find_next_case_clause_start(bytes, clause_header_end + 1, body_end)
                .unwrap_or(body_end);

            if is_case {
                let mut cursor = clause_start + 4;
                if let Some(target) =
                    parse_type_target(bytes, &mut cursor, export_types, package_locals)
                {
                    let mut bindings = FxHashMap::default();
                    bindings.insert(binding_name.clone(), target);
                    let clause_body = &bytes[clause_header_end + 1..clause_end];
                    extract_go_type_bindings(
                        clause_body,
                        export_types,
                        package_locals,
                        helper_targets,
                        &mut bindings,
                    );
                    accesses.extend(collect_bound_member_accesses(clause_body, &bindings));
                }
            }

            clause_start = clause_end;
        }

        i = body_end.saturating_add(1);
    }

    accesses
}

fn parse_type_switch_binding_name(bytes: &[u8], start: usize, end: usize) -> Option<String> {
    let header = std::str::from_utf8(&bytes[start..end]).ok()?;
    if !header.contains(".(type)") {
        return None;
    }
    let before_assign = header.split(":=").next()?;
    let binding = before_assign.split_whitespace().last()?;
    if binding.is_empty() {
        return None;
    }
    Some(binding.to_string())
}

fn find_case_clause_header_end(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut i = start;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;

    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'(' => paren_depth = paren_depth.saturating_add(1),
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth = bracket_depth.saturating_add(1),
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth = brace_depth.saturating_add(1),
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b':' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }

    None
}

fn find_next_case_clause_start(bytes: &[u8], mut i: usize, end: usize) -> Option<usize> {
    let mut depth = 0u32;
    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'{' => depth = depth.saturating_add(1),
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && (is_keyword_at(bytes, i, b"case") || is_keyword_at(bytes, i, b"default")) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn collect_bound_member_accesses(
    bytes: &[u8],
    bindings: &FxHashMap<String, String>,
) -> Vec<MemberAccess> {
    if bindings.is_empty() {
        return Vec::new();
    }

    let mut accesses = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let start = i;
        let Some(object) = parse_ident(bytes, &mut i) else {
            i = start.saturating_add(1);
            continue;
        };

        let Some(target) = bindings.get(object) else {
            continue;
        };
        if i >= bytes.len() || bytes[i] != b'.' {
            continue;
        }

        i += 1;
        let mut member_i = i;
        let Some(member) = parse_ident(bytes, &mut member_i) else {
            continue;
        };
        i = member_i;

        if is_go_exported(member) {
            accesses.push(MemberAccess {
                object: target.clone(),
                member: member.to_string(),
            });
        }
    }

    accesses
}

fn extract_go_type_bindings(
    bytes: &[u8],
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
    bindings: &mut FxHashMap<String, String>,
) {
    let mut i = 0;
    while i < bytes.len() {
        if let Some(next) = skip_string_or_rune(bytes, i, bytes.len()) {
            i = next;
            continue;
        }

        let start = i;
        let Some(first) = parse_ident(bytes, &mut i) else {
            i = start.saturating_add(1);
            continue;
        };

        if first == "var" {
            extract_var_type_binding(
                bytes,
                &mut i,
                export_types,
                package_locals,
                helper_targets,
                bindings,
            );
            continue;
        }

        let Some(name) = parse_assignment_target(first, bytes, start) else {
            continue;
        };
        let after_ident = skip_ascii_whitespace(bytes, i, bytes.len());
        if after_ident + 1 < bytes.len()
            && bytes[after_ident] == b':'
            && bytes[after_ident + 1] == b'='
        {
            let mut cursor = after_ident + 2;
            if let Some(target) = parse_type_target_from_expression(
                bytes,
                &mut cursor,
                export_types,
                package_locals,
                helper_targets,
            )
            .or_else(|| parse_bound_target_from_expression(bytes, &mut cursor, bindings))
            {
                bindings.insert(name.to_string(), target);
            }
            i = cursor;
            continue;
        }
        if after_ident < bytes.len() && bytes[after_ident] == b'=' {
            let mut cursor = after_ident + 1;
            if let Some(target) = parse_type_target_from_expression(
                bytes,
                &mut cursor,
                export_types,
                package_locals,
                helper_targets,
            )
            .or_else(|| parse_bound_target_from_expression(bytes, &mut cursor, bindings))
            {
                bindings.insert(name.to_string(), target);
            }
            i = cursor;
        }
    }
}

fn extract_var_type_binding(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
    bindings: &mut FxHashMap<String, String>,
) {
    skip_whitespace(bytes, i);
    let Some(name) = parse_ident(bytes, i) else {
        skip_to_line_end(bytes, i);
        return;
    };

    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    if cursor < bytes.len() && bytes[cursor] == b'*' {
        cursor += 1;
    }

    let annotated_target = parse_type_target(bytes, &mut cursor, export_types, package_locals);
    let after_annotation = skip_ascii_whitespace(bytes, cursor, bytes.len());
    if after_annotation < bytes.len() && bytes[after_annotation] == b'=' {
        let mut expr_cursor = after_annotation + 1;
        if let Some(target) = parse_type_target_from_expression(
            bytes,
            &mut expr_cursor,
            export_types,
            package_locals,
            helper_targets,
        )
        .or_else(|| parse_bound_target_from_expression(bytes, &mut expr_cursor, bindings))
        .or(annotated_target)
        {
            bindings.insert(name.to_string(), target);
        }
        *i = expr_cursor;
        return;
    }

    if let Some(target) = annotated_target {
        bindings.insert(name.to_string(), target);
        *i = cursor;
        return;
    }

    skip_to_line_end(bytes, i);
}

fn parse_assignment_target<'a>(ident: &'a str, bytes: &'a [u8], start: usize) -> Option<&'a str> {
    if previous_non_whitespace(bytes, start) == Some(b'.') {
        return None;
    }
    Some(ident)
}

fn parse_type_target_from_expression(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Option<String> {
    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    if let Some(target) = parse_parenthesized_target_from_expression(
        bytes,
        &mut cursor,
        export_types,
        package_locals,
        helper_targets,
    ) {
        *i = cursor;
        return Some(target);
    }
    cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    if cursor < bytes.len() && bytes[cursor] == b'&' {
        cursor += 1;
    }
    let expr_start = cursor;
    if let Some(target) =
        parse_composite_literal_target(bytes, &mut cursor, export_types, package_locals)
    {
        *i = cursor;
        return Some(target);
    }
    cursor = expr_start;
    if let Some(target) = parse_type_target(bytes, &mut cursor, export_types, package_locals) {
        *i = cursor;
        return Some(target);
    }
    cursor = expr_start;
    if let Some(target) = parse_constructor_target(bytes, &mut cursor, export_types, package_locals)
    {
        *i = cursor;
        return Some(target);
    }
    cursor = expr_start;
    if let Some(target) =
        parse_type_assertion_target(bytes, &mut cursor, export_types, package_locals)
    {
        *i = cursor;
        return Some(target);
    }
    cursor = expr_start;
    let target = parse_helper_call_target(
        bytes,
        &mut cursor,
        export_types,
        package_locals,
        helper_targets,
    )?;
    *i = cursor;
    Some(target)
}

fn parse_parenthesized_target_from_expression(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Option<String> {
    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    if cursor >= bytes.len() || bytes[cursor] != b'(' {
        return None;
    }
    cursor += 1;
    let target = parse_type_target_from_expression(
        bytes,
        &mut cursor,
        export_types,
        package_locals,
        helper_targets,
    )?;
    cursor = skip_ascii_whitespace(bytes, cursor, bytes.len());
    if cursor >= bytes.len() || bytes[cursor] != b')' {
        return None;
    }
    *i = cursor.saturating_add(1);
    Some(target)
}

fn parse_composite_literal_target(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
) -> Option<String> {
    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    let target = parse_type_target(bytes, &mut cursor, export_types, package_locals)?;
    let literal_open = skip_ascii_whitespace(bytes, cursor, bytes.len());
    if literal_open >= bytes.len() || bytes[literal_open] != b'{' {
        return None;
    }
    let literal_end = find_matching(bytes, literal_open, bytes.len(), b'{', b'}')?;
    *i = literal_end.saturating_add(1);
    Some(target)
}

fn parse_type_assertion_target(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
) -> Option<String> {
    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'.' && bytes[cursor + 1] == b'(' {
            let open = cursor + 1;
            let close = find_matching(bytes, open, bytes.len(), b'(', b')')?;
            let mut inner = open + 1;
            let target = parse_type_target(bytes, &mut inner, export_types, package_locals)?;
            *i = close.saturating_add(1);
            return Some(target);
        }
        match bytes[cursor] {
            b'\n' | b',' | b';' | b'}' => break,
            _ => cursor += 1,
        }
    }

    None
}

fn parse_type_target(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
) -> Option<String> {
    *i = skip_ascii_whitespace(bytes, *i, bytes.len());
    let first = parse_ident(bytes, i)?;

    if export_types.contains(first) {
        *i = skip_optional_go_type_arguments(bytes, *i, bytes.len())?;
        let next = skip_ascii_whitespace(bytes, *i, bytes.len());
        if next < bytes.len() && bytes[next] == b'(' {
            return None;
        }
        return Some(first.to_string());
    }

    if package_locals.contains(first) && *i < bytes.len() && bytes[*i] == b'.' {
        *i += 1;
        let ty = parse_ident(bytes, i)?;
        *i = skip_optional_go_type_arguments(bytes, *i, bytes.len())?;
        let next = skip_ascii_whitespace(bytes, *i, bytes.len());
        if next < bytes.len() && bytes[next] == b'(' {
            return None;
        }
        if is_go_exported(ty) {
            return Some(format!("{first}.{ty}"));
        }
    }

    None
}

fn parse_constructor_target(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
) -> Option<String> {
    *i = skip_ascii_whitespace(bytes, *i, bytes.len());

    let start = *i;
    let first = parse_ident(bytes, i)?;

    if first == "new" {
        let open = skip_ascii_whitespace(bytes, *i, bytes.len());
        if open >= bytes.len() || bytes[open] != b'(' {
            return None;
        }
        let mut cursor = open + 1;
        let target = parse_type_target(bytes, &mut cursor, export_types, package_locals)?;
        let close = find_matching(bytes, open, bytes.len(), b'(', b')')?;
        *i = close.saturating_add(1);
        return Some(target);
    }

    if package_locals.contains(first) && *i < bytes.len() && bytes[*i] == b'.' {
        *i += 1;
        let ctor_name = parse_ident(bytes, i)?;
        let type_name = ctor_name.strip_prefix("New")?;
        if !is_go_exported(type_name) {
            return None;
        }
        *i = skip_optional_go_type_arguments(bytes, *i, bytes.len())?;
        let call_open = skip_ascii_whitespace(bytes, *i, bytes.len());
        if call_open >= bytes.len() || bytes[call_open] != b'(' {
            return None;
        }
        let call_end = find_matching(bytes, call_open, bytes.len(), b'(', b')')?;
        *i = call_end.saturating_add(1);
        return Some(format!("{first}.{type_name}"));
    }

    let type_name = first.strip_prefix("New")?;
    if !is_go_exported(type_name) || !export_types.contains(type_name) {
        *i = start;
        return None;
    }
    *i = skip_optional_go_type_arguments(bytes, *i, bytes.len())?;
    let call_open = skip_ascii_whitespace(bytes, *i, bytes.len());
    if call_open >= bytes.len() || bytes[call_open] != b'(' {
        *i = start;
        return None;
    }
    let call_end = find_matching(bytes, call_open, bytes.len(), b'(', b')')?;
    *i = call_end.saturating_add(1);
    Some(type_name.to_string())
}

fn parse_bound_target_from_expression(
    bytes: &[u8],
    i: &mut usize,
    bindings: &FxHashMap<String, String>,
) -> Option<String> {
    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    if cursor < bytes.len() && bytes[cursor] == b'&' {
        cursor += 1;
    }
    let binding_start = cursor;
    let ident = parse_ident(bytes, &mut cursor)?;
    let target = bindings.get(ident)?.clone();
    let next = skip_ascii_whitespace(bytes, cursor, bytes.len());
    if next < bytes.len() && bytes[next] == b'.' {
        *i = binding_start;
        return None;
    }
    *i = cursor;
    Some(target)
}

fn parse_helper_call_target(
    bytes: &[u8],
    i: &mut usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Option<String> {
    let mut cursor = skip_ascii_whitespace(bytes, *i, bytes.len());
    let first = parse_ident(bytes, &mut cursor)?;
    if package_locals.contains(first) && cursor < bytes.len() && bytes[cursor] == b'.' {
        cursor += 1;
        let helper_name = parse_ident(bytes, &mut cursor)?;
        cursor = skip_optional_go_type_arguments(bytes, cursor, bytes.len())?;
        let call_open = skip_ascii_whitespace(bytes, cursor, bytes.len());
        if call_open >= bytes.len() || bytes[call_open] != b'(' {
            return None;
        }
        let call_end = find_matching(bytes, call_open, bytes.len(), b'(', b')')?;
        let qualified_name = format!("{first}.{helper_name}");
        if let Some(target) = helper_targets.qualified_fixed.get(&qualified_name) {
            *i = call_end.saturating_add(1);
            return Some(target.clone());
        }
        if let Some(arg_index) = helper_targets
            .qualified_passthrough_arg_index
            .get(&qualified_name)
            && let Some(target) = parse_helper_passthrough_arg_target(
                bytes,
                call_open,
                *arg_index,
                export_types,
                package_locals,
                helper_targets,
            )
        {
            *i = call_end.saturating_add(1);
            return Some(target);
        }
        *i = call_end.saturating_add(1);
        return None;
    }
    let helper_name = first;
    cursor = skip_optional_go_type_arguments(bytes, cursor, bytes.len())?;
    let call_open = skip_ascii_whitespace(bytes, cursor, bytes.len());
    if call_open >= bytes.len() || bytes[call_open] != b'(' {
        return None;
    }
    let call_end = find_matching(bytes, call_open, bytes.len(), b'(', b')')?;
    if let Some(target) = helper_targets.fixed.get(helper_name) {
        *i = call_end.saturating_add(1);
        return Some(target.clone());
    }
    if let Some(arg_index) = helper_targets.passthrough_arg_index.get(helper_name)
        && let Some(target) = parse_helper_passthrough_arg_target(
            bytes,
            call_open,
            *arg_index,
            export_types,
            package_locals,
            helper_targets,
        )
    {
        *i = call_end.saturating_add(1);
        return Some(target);
    }
    *i = call_end.saturating_add(1);
    None
}

#[derive(Default)]
struct GoHelperTargets {
    fixed: FxHashMap<String, String>,
    passthrough_arg_index: FxHashMap<String, usize>,
    qualified_fixed: FxHashMap<String, String>,
    qualified_passthrough_arg_index: FxHashMap<String, usize>,
}

fn collect_go_helper_targets(
    path: &Path,
    bytes: &[u8],
    imports: &[ImportInfo],
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
) -> GoHelperTargets {
    let mut helper_targets =
        collect_local_helper_return_targets(bytes, export_types, package_locals);
    collect_imported_helper_return_targets(path, imports, &mut helper_targets);
    helper_targets
}

fn collect_local_helper_return_targets(
    bytes: &[u8],
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
) -> GoHelperTargets {
    let mut helper_candidates = Vec::new();
    let mut i = 0;
    let mut depth = 0u32;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }

        if depth > 0 || !(i == 0 || bytes[i - 1] == b'\n') {
            i += 1;
            continue;
        }

        let line_start = i;
        let func_kw = skip_ascii_whitespace(bytes, i, bytes.len());
        if !is_keyword_at(bytes, func_kw, b"func") {
            i = line_start.saturating_add(1);
            continue;
        }
        let after_func = skip_ascii_whitespace(bytes, func_kw + 4, bytes.len());
        if after_func < bytes.len() && bytes[after_func] == b'(' {
            i = line_start.saturating_add(1);
            continue;
        }

        let Some(function) = parse_function_at(bytes, func_kw, bytes.len()) else {
            i = line_start.saturating_add(1);
            continue;
        };
        let Some((name_start, name_end)) = function.name else {
            i = function.body_end.saturating_add(1);
            continue;
        };
        let Some(name) = std::str::from_utf8(&bytes[name_start..name_end]).ok() else {
            i = function.body_end.saturating_add(1);
            continue;
        };

        helper_candidates.push((
            name.to_string(),
            helper_param_names(bytes, function.params_start, function.params_end),
            function.params_end.saturating_add(1),
            function.body_start.saturating_add(1),
            function.body_end,
        ));

        i = function.body_end.saturating_add(1);
    }

    let mut helper_targets = GoHelperTargets::default();
    loop {
        let mut changed = false;
        for (name, param_names, signature_start, body_start, body_end) in &helper_candidates {
            let mut return_cursor = *signature_start;
            let signature_target =
                parse_type_target(bytes, &mut return_cursor, export_types, package_locals);
            let (body_target, passthrough_arg_index) = analyze_helper_return_behavior(
                bytes,
                *body_start,
                *body_end,
                export_types,
                package_locals,
                &helper_targets,
                param_names,
            );
            if let Some(arg_index) = passthrough_arg_index
                && body_target.is_none()
            {
                if !helper_targets.fixed.contains_key(name)
                    && helper_targets.passthrough_arg_index.get(name) != Some(&arg_index)
                {
                    helper_targets
                        .passthrough_arg_index
                        .insert(name.clone(), arg_index);
                    changed = true;
                }
                continue;
            }
            let resolved_target = body_target.clone().or_else(|| signature_target.clone());
            if let Some(target) = resolved_target
                && helper_targets.fixed.get(name) != Some(&target)
            {
                helper_targets.fixed.insert(name.clone(), target);
                helper_targets.passthrough_arg_index.remove(name);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    helper_targets
}

#[derive(Debug, Deserialize)]
struct GoListPackage {
    #[serde(rename = "Dir")]
    dir: String,
    #[serde(rename = "GoFiles", default)]
    go_files: Vec<String>,
}

fn collect_imported_helper_return_targets(
    path: &Path,
    imports: &[ImportInfo],
    helper_targets: &mut GoHelperTargets,
) {
    let Some(current_dir) = path.parent() else {
        return;
    };

    for import in imports {
        if !matches!(import.imported_name, ImportedName::Namespace) || import.local_name.is_empty()
        {
            continue;
        }

        let output = match Command::new("go")
            .arg("list")
            .arg("-json")
            .arg(&import.source)
            .current_dir(current_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        let Ok(pkg) = serde_json::from_slice::<GoListPackage>(&output.stdout) else {
            continue;
        };
        if pkg.go_files.is_empty() {
            continue;
        }

        let mut file_sources = Vec::new();
        let mut package_export_type_names: FxHashSet<String> = FxHashSet::default();
        for go_file in &pkg.go_files {
            let file_path = Path::new(&pkg.dir).join(go_file);
            let Ok(source) = std::fs::read_to_string(&file_path) else {
                continue;
            };
            let stripped = strip_go_comments(&source);
            let file_exports = extract_exports(&stripped, &source, false);
            for export in &file_exports {
                if export.is_type_only
                    && let ExportName::Named(name) = &export.name
                {
                    package_export_type_names.insert(name.clone());
                }
            }
            file_sources.push(stripped);
        }
        if file_sources.is_empty() {
            continue;
        }
        let package_export_types: FxHashSet<&str> = package_export_type_names
            .iter()
            .map(std::string::String::as_str)
            .collect();

        for stripped in &file_sources {
            let imported_local_names = extract_imports(stripped)
                .into_iter()
                .filter(|imp| {
                    matches!(imp.imported_name, ImportedName::Namespace)
                        && !imp.local_name.is_empty()
                })
                .map(|imp| imp.local_name)
                .collect::<Vec<_>>();
            let imported_package_locals: FxHashSet<&str> = imported_local_names
                .iter()
                .map(std::string::String::as_str)
                .collect();
            let imported_targets = collect_local_helper_return_targets(
                stripped.as_bytes(),
                &package_export_types,
                &imported_package_locals,
            );
            for (name, target) in imported_targets.fixed {
                let qualified_target = if target.contains('.') {
                    target
                } else {
                    format!("{}.{}", import.local_name, target)
                };
                helper_targets
                    .qualified_fixed
                    .insert(format!("{}.{}", import.local_name, name), qualified_target);
            }
            for (name, arg_index) in imported_targets.passthrough_arg_index {
                helper_targets
                    .qualified_passthrough_arg_index
                    .insert(format!("{}.{}", import.local_name, name), arg_index);
            }
        }
    }
}

fn analyze_helper_return_behavior(
    bytes: &[u8],
    start: usize,
    end: usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
    param_names: &[String],
) -> (Option<String>, Option<usize>) {
    let mut bindings: FxHashMap<String, String> = FxHashMap::default();
    for (index, param_name) in param_names.iter().enumerate() {
        bindings.insert(param_name.clone(), helper_param_sentinel(index));
    }
    extract_go_type_bindings(
        &bytes[start..end],
        export_types,
        package_locals,
        helper_targets,
        &mut bindings,
    );

    let mut i = start;
    let mut resolved_target: Option<String> = None;
    let mut saw_unresolvable_direct_return = false;
    let mut passthrough_arg_index: Option<usize> = None;
    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        if is_keyword_at(bytes, i, b"return") {
            let mut cursor = i + 6;
            if let Some(target) = parse_type_target_from_expression(
                bytes,
                &mut cursor,
                export_types,
                package_locals,
                helper_targets,
            ) {
                if let Some(index) = helper_param_index(&target) {
                    match passthrough_arg_index {
                        None => passthrough_arg_index = Some(index),
                        Some(existing) if existing == index => {}
                        Some(_) => return (None, None),
                    }
                } else {
                    match &resolved_target {
                        None => resolved_target = Some(target),
                        Some(existing) if *existing == target => {}
                        Some(_) => return (None, None),
                    }
                }
            } else if let Some(target) =
                parse_bound_target_from_expression(bytes, &mut cursor, &bindings)
            {
                if let Some(index) = helper_param_index(&target) {
                    match passthrough_arg_index {
                        None => passthrough_arg_index = Some(index),
                        Some(existing) if existing == index => {}
                        Some(_) => return (None, None),
                    }
                } else {
                    match &resolved_target {
                        None => resolved_target = Some(target),
                        Some(existing) if *existing == target => {}
                        Some(_) => return (None, None),
                    }
                }
            } else {
                saw_unresolvable_direct_return = true;
            }
        }

        i += 1;
    }

    if resolved_target.is_some() && passthrough_arg_index.is_some() {
        return (None, None);
    }
    if resolved_target.is_none()
        && passthrough_arg_index.is_none()
        && saw_unresolvable_direct_return
    {
        return (None, None);
    }
    (resolved_target, passthrough_arg_index)
}

fn parse_helper_passthrough_arg_target(
    bytes: &[u8],
    call_open: usize,
    arg_index: usize,
    export_types: &FxHashSet<&str>,
    package_locals: &FxHashSet<&str>,
    helper_targets: &GoHelperTargets,
) -> Option<String> {
    let call_end = find_matching(bytes, call_open, bytes.len(), b'(', b')')?;
    let mut current_index = 0;
    let mut arg_start = skip_ascii_whitespace(bytes, call_open + 1, call_end);
    while arg_start < call_end {
        let arg_end = skip_go_argument(bytes, arg_start, call_end);
        if current_index == arg_index {
            let mut cursor = arg_start;
            return parse_type_target_from_expression(
                bytes,
                &mut cursor,
                export_types,
                package_locals,
                helper_targets,
            );
        }
        current_index += 1;
        arg_start = skip_ascii_whitespace(bytes, arg_end.saturating_add(1), call_end);
    }
    None
}

fn skip_optional_go_type_arguments(bytes: &[u8], i: usize, end: usize) -> Option<usize> {
    let cursor = skip_ascii_whitespace(bytes, i, end);
    if cursor < end && bytes[cursor] == b'[' {
        return Some(find_matching(bytes, cursor, end, b'[', b']')?.saturating_add(1));
    }
    Some(cursor)
}

fn helper_param_names(bytes: &[u8], params_start: usize, params_end: usize) -> Vec<String> {
    if params_start + 1 >= params_end {
        return Vec::new();
    }
    let mut names = Vec::new();
    let mut i = params_start + 1;
    while i < params_end {
        let field_end = skip_go_argument(bytes, i, params_end);
        let mut cursor = skip_ascii_whitespace(bytes, i, field_end);
        if let Some(name) = parse_ident(bytes, &mut cursor) {
            names.push(name.to_string());
        }
        i = field_end.saturating_add(1);
    }
    names
}

fn helper_param_sentinel(index: usize) -> String {
    format!("__param{index}")
}

fn helper_param_index(target: &str) -> Option<usize> {
    target.strip_prefix("__param")?.parse().ok()
}

fn previous_non_whitespace(bytes: &[u8], mut index: usize) -> Option<u8> {
    while index > 0 {
        index -= 1;
        if !bytes[index].is_ascii_whitespace() {
            return Some(bytes[index]);
        }
    }
    None
}

fn previous_identifier_introduces_declaration(prev_ident: Option<&str>) -> bool {
    matches!(
        prev_ident,
        Some("func" | "type" | "var" | "const" | "package" | "import")
    )
}

fn is_declaration_keyword(ident: &str) -> bool {
    matches!(
        ident,
        "func"
            | "type"
            | "var"
            | "const"
            | "package"
            | "import"
            | "if"
            | "else"
            | "for"
            | "switch"
            | "select"
            | "case"
            | "default"
            | "return"
            | "go"
            | "defer"
            | "struct"
            | "interface"
            | "map"
            | "chan"
            | "range"
    )
}

fn next_tokens_are_short_declaration(bytes: &[u8], mut index: usize) -> bool {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index + 1 < bytes.len() && bytes[index] == b':' && bytes[index + 1] == b'='
}

fn attach_receiver_members(exports: &mut [ExportInfo], stripped: &str, original: &str) {
    let exported_types: FxHashSet<String> = exports
        .iter()
        .filter(|export| export.is_type_only)
        .filter_map(|export| match &export.name {
            ExportName::Named(name) => Some(name.clone()),
            ExportName::Default => None,
        })
        .collect();
    if exported_types.is_empty() {
        return;
    }

    let mut members_by_type = FxHashMap::<String, Vec<MemberInfo>>::default();
    collect_receiver_methods(
        stripped.as_bytes(),
        original,
        &exported_types,
        &mut members_by_type,
    );
    collect_exported_type_members(
        stripped.as_bytes(),
        original,
        &exported_types,
        &mut members_by_type,
    );

    for export in exports {
        let ExportName::Named(name) = &export.name else {
            continue;
        };
        if let Some(members) = members_by_type.remove(name) {
            export.members = members;
        }
        export.local_name = Some(name.clone());
    }
}

fn collect_receiver_methods(
    bytes: &[u8],
    original: &str,
    exported_types: &FxHashSet<String>,
    members_by_type: &mut FxHashMap<String, Vec<MemberInfo>>,
) {
    let mut i = 0;
    let mut depth = 0u32;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }

        if depth > 0 || !(i == 0 || bytes[i - 1] == b'\n') {
            i += 1;
            continue;
        }

        let line_start = i;
        skip_whitespace(bytes, &mut i);
        if !try_consume(bytes, &mut i, b"func") || i >= bytes.len() || bytes[i] != b'(' {
            i = line_start.saturating_add(1);
            continue;
        }

        let receiver_start = i;
        i += 1;
        skip_whitespace(bytes, &mut i);
        let _receiver_name = parse_ident(bytes, &mut i);
        skip_whitespace(bytes, &mut i);
        if i < bytes.len() && bytes[i] == b'*' {
            i += 1;
        }
        let Some(receiver_type) = parse_ident(bytes, &mut i) else {
            i = line_start.saturating_add(1);
            continue;
        };
        let receiver_type = receiver_type.split('[').next().unwrap_or(receiver_type);
        let Some(close_paren) = find_matching(bytes, receiver_start, bytes.len(), b'(', b')')
        else {
            i = line_start.saturating_add(1);
            continue;
        };
        i = close_paren.saturating_add(1);
        skip_whitespace(bytes, &mut i);
        let member_start = i;
        let Some(method_name) = parse_ident(bytes, &mut i) else {
            i = line_start.saturating_add(1);
            continue;
        };
        if !is_go_exported(method_name) || !exported_types.contains(receiver_type) {
            skip_to_line_end(bytes, &mut i);
            continue;
        }

        members_by_type
            .entry(receiver_type.to_string())
            .or_default()
            .push(MemberInfo {
                name: method_name.to_string(),
                kind: MemberKind::ClassMethod,
                span: byte_offset_span(member_start, original),
                has_decorator: false,
            });
        skip_to_line_end(bytes, &mut i);
    }
}

fn collect_exported_type_members(
    bytes: &[u8],
    original: &str,
    exported_types: &FxHashSet<String>,
    members_by_type: &mut FxHashMap<String, Vec<MemberInfo>>,
) {
    let mut i = 0;
    let mut depth = 0u32;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }

        if depth > 0 || !(i == 0 || bytes[i - 1] == b'\n') {
            i += 1;
            continue;
        }

        let line_start = i;
        skip_whitespace(bytes, &mut i);
        if !try_consume(bytes, &mut i, b"type") {
            i = line_start.saturating_add(1);
            continue;
        }
        let Some(type_name) = parse_ident(bytes, &mut i) else {
            i = line_start.saturating_add(1);
            continue;
        };
        if !exported_types.contains(type_name) {
            skip_to_line_end(bytes, &mut i);
            continue;
        }
        skip_whitespace(bytes, &mut i);
        if i < bytes.len() && bytes[i] == b'[' {
            let Some(end) = find_matching(bytes, i, bytes.len(), b'[', b']') else {
                i = line_start.saturating_add(1);
                continue;
            };
            i = end.saturating_add(1);
            skip_whitespace(bytes, &mut i);
        }

        let Some(kind) = parse_ident(bytes, &mut i) else {
            i = line_start.saturating_add(1);
            continue;
        };
        if !matches!(kind, "struct" | "interface") {
            skip_to_line_end(bytes, &mut i);
            continue;
        }
        let brace_start = skip_ascii_whitespace(bytes, i, bytes.len());
        if brace_start >= bytes.len() || bytes[brace_start] != b'{' {
            i = line_start.saturating_add(1);
            continue;
        }
        let Some(brace_end) = find_matching(bytes, brace_start, bytes.len(), b'{', b'}') else {
            i = line_start.saturating_add(1);
            continue;
        };
        let mut cursor = brace_start + 1;
        while cursor < brace_end {
            if let Some(next) = skip_string_or_rune(bytes, cursor, brace_end) {
                cursor = next;
                continue;
            }
            let member_start = cursor;
            let Some(name) = parse_ident(bytes, &mut cursor) else {
                cursor = member_start.saturating_add(1);
                continue;
            };
            if !is_go_exported(name) || is_declaration_keyword(name) {
                continue;
            }
            let member_kind = if cursor < brace_end && bytes[cursor] == b'(' {
                MemberKind::ClassMethod
            } else {
                MemberKind::ClassProperty
            };
            members_by_type
                .entry(type_name.to_string())
                .or_default()
                .push(MemberInfo {
                    name: name.to_string(),
                    kind: member_kind,
                    span: byte_offset_span(member_start, original),
                    has_decorator: false,
                });
        }
        i = brace_end.saturating_add(1);
    }

    for members in members_by_type.values_mut() {
        members.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| (a.kind as u8).cmp(&(b.kind as u8)))
        });
        members.dedup_by(|a, b| a.name == b.name && a.kind == b.kind);
    }
}

struct GoImportBindingUsage {
    unused: Vec<String>,
    value_referenced: Vec<String>,
}

fn compute_go_import_binding_usage(
    imports: &[ImportInfo],
    member_accesses: &[MemberAccess],
) -> GoImportBindingUsage {
    let used_locals: FxHashSet<&str> = member_accesses
        .iter()
        .filter_map(|access| {
            if access.object.is_empty() {
                None
            } else {
                Some(
                    access
                        .object
                        .split_once('.')
                        .map_or(access.object.as_str(), |(local, _)| local),
                )
            }
        })
        .collect();
    let mut unused = Vec::new();
    let mut value_referenced = Vec::new();

    for import in imports {
        if import.local_name.is_empty() || matches!(import.imported_name, ImportedName::SideEffect)
        {
            continue;
        }

        if used_locals.contains(import.local_name.as_str()) {
            value_referenced.push(import.local_name.clone());
        } else {
            unused.push(import.local_name.clone());
        }
    }

    unused.sort_unstable();
    unused.dedup();
    value_referenced.sort_unstable();
    value_referenced.dedup();

    GoImportBindingUsage {
        unused,
        value_referenced,
    }
}

const GO_FLAG_ENV_PREFIXES: &[&str] = &["FEATURE_", "ENABLE_", "FF_", "FLAG_", "TOGGLE_"];

const GO_SDK_PATTERNS: &[(&str, usize, &str)] = &[
    ("Variation", 0, "LaunchDarkly"),
    ("BoolVariation", 0, "LaunchDarkly"),
    ("StringVariation", 0, "LaunchDarkly"),
    ("IntVariation", 0, "LaunchDarkly"),
    ("Float64Variation", 0, "LaunchDarkly"),
    ("JSONVariation", 0, "LaunchDarkly"),
    ("UseGate", 0, "Statsig"),
    ("CheckGate", 0, "Statsig"),
    ("UseExperiment", 0, "Statsig"),
    ("UseConfig", 0, "Statsig"),
    ("IsEnabled", 0, "Unleash"),
    ("GetVariant", 0, "Unleash"),
    ("GetFeatureValue", 0, "GrowthBook"),
    ("GetTreatment", 0, "Split"),
    ("GetValue", 0, ""),
    ("GetValueAsync", 0, "ConfigCat"),
    ("HasFeature", 0, "Flagsmith"),
];

fn extract_go_flags(
    stripped: &str,
    imports: &[ImportInfo],
    line_offsets: &[u32],
    extra_sdk_patterns: &[(String, usize, String)],
    extra_env_prefixes: &[String],
) -> Vec<FlagUse> {
    let bytes = stripped.as_bytes();
    let mut flags = scan_go_flags_in_range(
        bytes,
        0,
        bytes.len(),
        imports,
        line_offsets,
        extra_sdk_patterns,
        extra_env_prefixes,
        None,
    );

    let mut i = 0;
    while i < bytes.len() {
        if let Some(next) = skip_string_or_rune(bytes, i, bytes.len()) {
            i = next;
            continue;
        }

        if is_keyword_at(bytes, i, b"if") {
            let body_start =
                find_statement_block_start(bytes, i + 2, bytes.len()).unwrap_or(bytes.len());
            let body_end =
                find_matching(bytes, body_start, bytes.len(), b'{', b'}').unwrap_or(body_start);
            let guard = Some((
                u32::try_from(i).unwrap_or(u32::MAX),
                u32::try_from(body_end).unwrap_or(u32::MAX),
            ));
            let header_flags = scan_go_flags_in_range(
                bytes,
                i + 2,
                body_start,
                imports,
                line_offsets,
                extra_sdk_patterns,
                extra_env_prefixes,
                guard,
            );
            merge_flag_uses(&mut flags, header_flags);
            i = body_end.saturating_add(1);
            continue;
        }

        i += 1;
    }

    flags
}

#[allow(
    clippy::too_many_arguments,
    reason = "Go flag scanning needs source spans, imports, custom patterns, and optional guard context together"
)]
fn scan_go_flags_in_range(
    bytes: &[u8],
    start: usize,
    end: usize,
    imports: &[ImportInfo],
    line_offsets: &[u32],
    extra_sdk_patterns: &[(String, usize, String)],
    extra_env_prefixes: &[String],
    guard: Option<(u32, u32)>,
) -> Vec<FlagUse> {
    let os_locals: FxHashSet<&str> = imports
        .iter()
        .filter(|import| import.source == "os")
        .map(|import| import.local_name.as_str())
        .filter(|name| !name.is_empty())
        .collect();

    let mut flags = Vec::new();
    let mut i = start;
    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        let ident_start = i;
        let Some(first_ident) = parse_ident(bytes, &mut i) else {
            i = ident_start.saturating_add(1);
            continue;
        };

        let mut func_name = first_ident;
        let mut is_os_call = false;
        if i < end && bytes[i] == b'.' {
            i += 1;
            let mut member_i = i;
            if let Some(member) = parse_ident(bytes, &mut member_i) {
                func_name = member;
                is_os_call = os_locals.contains(first_ident);
                i = member_i;
            }
        }

        let call_open = skip_ascii_whitespace(bytes, i, end);
        if call_open >= end || bytes[call_open] != b'(' {
            continue;
        }

        if is_os_call
            && matches!(func_name, "Getenv" | "GetEnv" | "LookupEnv")
            && let Some(flag_name) = extract_go_string_arg(bytes, call_open, end, 0)
            && GO_FLAG_ENV_PREFIXES
                .iter()
                .copied()
                .chain(extra_env_prefixes.iter().map(String::as_str))
                .any(|prefix| flag_name.starts_with(prefix))
        {
            let (line, col) = byte_offset_to_line_col(
                line_offsets,
                u32::try_from(ident_start).unwrap_or(u32::MAX),
            );
            flags.push(FlagUse {
                flag_name,
                kind: FlagUseKind::EnvVar,
                line,
                col,
                guard_span_start: guard.map(|(s, _)| s),
                guard_span_end: guard.map(|(_, e)| e),
                sdk_name: None,
            });
            continue;
        }

        for (pattern_name, arg_idx, provider) in GO_SDK_PATTERNS {
            if func_name == *pattern_name
                && let Some(flag_name) = extract_go_string_arg(bytes, call_open, end, *arg_idx)
            {
                let (line, col) = byte_offset_to_line_col(
                    line_offsets,
                    u32::try_from(ident_start).unwrap_or(u32::MAX),
                );
                flags.push(FlagUse {
                    flag_name,
                    kind: FlagUseKind::SdkCall,
                    line,
                    col,
                    guard_span_start: guard.map(|(s, _)| s),
                    guard_span_end: guard.map(|(_, e)| e),
                    sdk_name: if provider.is_empty() {
                        None
                    } else {
                        Some((*provider).to_string())
                    },
                });
                break;
            }
        }

        for (pattern_name, arg_idx, provider) in extra_sdk_patterns {
            if func_name == pattern_name
                && let Some(flag_name) = extract_go_string_arg(bytes, call_open, end, *arg_idx)
            {
                let (line, col) = byte_offset_to_line_col(
                    line_offsets,
                    u32::try_from(ident_start).unwrap_or(u32::MAX),
                );
                flags.push(FlagUse {
                    flag_name,
                    kind: FlagUseKind::SdkCall,
                    line,
                    col,
                    guard_span_start: guard.map(|(s, _)| s),
                    guard_span_end: guard.map(|(_, e)| e),
                    sdk_name: if provider.is_empty() {
                        None
                    } else {
                        Some(provider.clone())
                    },
                });
                break;
            }
        }
    }

    flags
}

/// Extract Go feature flags from source text with optional custom SDK patterns
/// and environment variable prefixes.
pub fn extract_go_flags_from_source(
    source: &str,
    path: &Path,
    extra_sdk_patterns: &[(String, usize, String)],
    extra_env_prefixes: &[String],
) -> Vec<FlagUse> {
    if !is_go_file(path) {
        return Vec::new();
    }
    let stripped = strip_go_comments(source);
    let imports = extract_imports(&stripped);
    let line_offsets = compute_line_offsets(source);
    extract_go_flags(
        &stripped,
        &imports,
        &line_offsets,
        extra_sdk_patterns,
        extra_env_prefixes,
    )
}

fn extract_go_string_arg(
    bytes: &[u8],
    call_open: usize,
    end: usize,
    target_index: usize,
) -> Option<String> {
    let call_end = find_matching(bytes, call_open, end, b'(', b')')?;
    let mut i = call_open + 1;
    let mut arg_index = 0usize;

    while i < call_end {
        i = skip_ascii_whitespace(bytes, i, call_end);
        if i >= call_end {
            break;
        }

        let arg_start = i;
        let arg_end = skip_go_argument(bytes, i, call_end);
        if arg_index == target_index {
            return extract_go_string_literal(bytes, arg_start, arg_end);
        }

        i = skip_ascii_whitespace(bytes, arg_end, call_end);
        if i < call_end && bytes[i] == b',' {
            i += 1;
            arg_index += 1;
            continue;
        }
        break;
    }

    None
}

fn skip_go_argument(bytes: &[u8], mut i: usize, end: usize) -> usize {
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;

    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        match bytes[i] {
            b'(' => paren_depth = paren_depth.saturating_add(1),
            b')' | b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return i;
            }
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth = bracket_depth.saturating_add(1),
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth = brace_depth.saturating_add(1),
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }

    end
}

fn extract_go_string_literal(bytes: &[u8], start: usize, end: usize) -> Option<String> {
    let start = skip_ascii_whitespace(bytes, start, end);
    if start >= end || !matches!(bytes[start], b'"' | b'`') {
        return None;
    }
    let quote = bytes[start];
    let literal_end = if quote == b'`' {
        bytes[start + 1..end]
            .iter()
            .position(|byte| *byte == b'`')
            .map(|offset| start + 1 + offset)?
    } else {
        let mut i = start + 1;
        while i < end {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                break;
            }
            i += 1;
        }
        if i >= end {
            return None;
        }
        i
    };

    std::str::from_utf8(&bytes[start + 1..literal_end])
        .ok()
        .map(ToString::to_string)
}

fn merge_flag_uses(existing: &mut Vec<FlagUse>, incoming: Vec<FlagUse>) {
    for flag in incoming {
        if let Some(current) = existing.iter_mut().find(|candidate| {
            candidate.line == flag.line
                && candidate.col == flag.col
                && candidate.flag_name == flag.flag_name
                && candidate.kind == flag.kind
        }) {
            if current.guard_span_start.is_none() {
                current.guard_span_start = flag.guard_span_start;
                current.guard_span_end = flag.guard_span_end;
            }
        } else {
            existing.push(flag);
        }
    }
}

fn extract_local_type_declarations(stripped: &str, original: &str) -> Vec<LocalTypeDeclaration> {
    let bytes = stripped.as_bytes();
    let mut declarations = Vec::new();
    let mut i = 0;
    let mut depth = 0u32;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }

        if depth > 0 {
            i += 1;
            continue;
        }

        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if !at_line_start {
            i += 1;
            continue;
        }

        let mut line_i = i;
        while line_i < bytes.len() && (bytes[line_i] == b' ' || bytes[line_i] == b'\t') {
            line_i += 1;
        }
        if !try_consume(bytes, &mut line_i, b"type") {
            i += 1;
            continue;
        }

        if line_i < bytes.len() && bytes[line_i] == b'(' {
            line_i += 1;
            while line_i < bytes.len() && bytes[line_i] != b')' {
                skip_whitespace(bytes, &mut line_i);
                if line_i < bytes.len() && bytes[line_i] == b'\n' {
                    line_i += 1;
                    continue;
                }
                let name_start = line_i;
                if let Some(name) = parse_ident(bytes, &mut line_i) {
                    declarations.push(LocalTypeDeclaration {
                        name: name.to_string(),
                        span: byte_offset_span(name_start, original),
                    });
                }
                skip_to_line_end(bytes, &mut line_i);
            }
            i = line_i.saturating_add(1);
            continue;
        }

        let name_start = line_i;
        if let Some(name) = parse_ident(bytes, &mut line_i) {
            declarations.push(LocalTypeDeclaration {
                name: name.to_string(),
                span: byte_offset_span(name_start, original),
            });
        }
        i = line_i;
    }

    declarations
}

fn extract_public_signature_type_references(
    stripped: &str,
    local_type_declarations: &[LocalTypeDeclaration],
) -> Vec<PublicSignatureTypeReference> {
    let local_type_names: FxHashSet<&str> = local_type_declarations
        .iter()
        .map(|declaration| declaration.name.as_str())
        .collect();
    if local_type_names.is_empty() {
        return Vec::new();
    }

    let bytes = stripped.as_bytes();
    let mut references = Vec::new();
    let mut i = 0;
    let mut depth = 0u32;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }

        if depth > 0 {
            i += 1;
            continue;
        }

        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if !at_line_start {
            i += 1;
            continue;
        }

        let line_start = i;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'\n' {
            i = i.saturating_add(1);
            continue;
        }

        if try_consume(bytes, &mut i, b"func") {
            if let Some((export_name, range_start, range_end)) =
                parse_exported_func_signature(bytes, i, bytes.len())
            {
                references.extend(collect_matching_signature_refs(
                    bytes,
                    export_name,
                    range_start,
                    range_end,
                    &local_type_names,
                ));
                i = range_end;
                continue;
            }
        } else if try_consume(bytes, &mut i, b"type")
            && let Some((export_name, range_start, range_end)) =
                parse_exported_type_signature(bytes, i, bytes.len())
        {
            references.extend(collect_matching_signature_refs(
                bytes,
                export_name,
                range_start,
                range_end,
                &local_type_names,
            ));
            i = range_end;
            continue;
        }

        i = line_start.saturating_add(1);
    }

    references
}

fn parse_exported_func_signature(
    bytes: &[u8],
    mut i: usize,
    end: usize,
) -> Option<(&str, usize, usize)> {
    if i < end && bytes[i] == b'(' {
        skip_to_line_end(bytes, &mut i);
        return None;
    }
    let name = parse_ident(bytes, &mut i)?;
    if !is_go_exported(name) {
        skip_to_line_end(bytes, &mut i);
        return None;
    }
    let sig_start = skip_ascii_whitespace(bytes, i, end);
    let mut cursor = sig_start;
    if cursor < end && bytes[cursor] == b'[' {
        cursor = find_matching(bytes, cursor, end, b'[', b']')?.saturating_add(1);
    }
    let body_start = find_statement_block_start(bytes, cursor, end)?;
    Some((name, sig_start, body_start))
}

fn parse_exported_type_signature(
    bytes: &[u8],
    mut i: usize,
    end: usize,
) -> Option<(&str, usize, usize)> {
    let name = parse_ident(bytes, &mut i)?;
    if !is_go_exported(name) {
        skip_to_line_end(bytes, &mut i);
        return None;
    }
    let mut sig_start = skip_ascii_whitespace(bytes, i, end);
    if sig_start < end && bytes[sig_start] == b'[' {
        sig_start = find_matching(bytes, sig_start, end, b'[', b']')?.saturating_add(1);
    }
    let sig_start = skip_ascii_whitespace(bytes, sig_start, end);

    let mut cursor = sig_start;
    while cursor < end && bytes[cursor] != b'\n' {
        if bytes[cursor] == b'{' {
            let body_end = find_matching(bytes, cursor, end, b'{', b'}')?;
            return Some((name, sig_start, body_end));
        }
        cursor += 1;
    }
    Some((name, sig_start, cursor))
}

fn collect_matching_signature_refs(
    bytes: &[u8],
    export_name: &str,
    start: usize,
    end: usize,
    local_type_names: &FxHashSet<&str>,
) -> Vec<PublicSignatureTypeReference> {
    let mut refs = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let ident_start = cursor;
        let Some(ident) = parse_ident(bytes, &mut cursor) else {
            cursor = ident_start.saturating_add(1);
            continue;
        };

        if ident != export_name && local_type_names.contains(ident) {
            refs.push(PublicSignatureTypeReference {
                export_name: export_name.to_string(),
                type_name: ident.to_string(),
                span: Span::new(
                    u32::try_from(ident_start).unwrap_or(u32::MAX),
                    u32::try_from(cursor).unwrap_or(u32::MAX),
                ),
            });
        }
    }
    refs
}

// ── Export extraction ─────────────────────────────────────────────────────────

/// Scan the stripped source for top-level exported declarations.
///
/// A declaration is "top-level" when it appears at brace depth 0 and "exported"
/// when its first identifier begins with an uppercase Unicode letter (Go's rule).
fn extract_exports(stripped: &str, original: &str, is_test_file: bool) -> Vec<ExportInfo> {
    let bytes = stripped.as_bytes();
    let len = bytes.len();
    let mut exports: Vec<ExportInfo> = Vec::new();

    let mut i = 0;
    let mut depth: u32 = 0; // brace nesting depth

    while i < len {
        let b = bytes[i];

        // Track brace depth.
        if b == b'{' {
            depth += 1;
            i += 1;
            continue;
        }
        if b == b'}' {
            depth = depth.saturating_sub(1);
            i += 1;
            continue;
        }

        // Only care about declarations at depth 0.
        if depth > 0 {
            i += 1;
            continue;
        }

        // Look for declaration keywords at line start (optionally preceded by whitespace).
        // We use a simple "at line start or after newline" heuristic.
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if !at_line_start {
            i += 1;
            continue;
        }

        // Skip leading whitespace on the line (Go top-level decls have no indentation,
        // but be lenient).
        let line_start = i;
        while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i >= len || bytes[i] == b'\n' {
            if i < len {
                i += 1;
            }
            continue;
        }

        // Try to match a keyword.
        if try_consume(bytes, &mut i, b"func") {
            if let Some(info) = parse_func_decl(bytes, &mut i, line_start, original, is_test_file) {
                exports.push(info);
            }
            continue;
        }
        if try_consume(bytes, &mut i, b"type") {
            if let Some(info) = parse_type_decl(bytes, &mut i, line_start, original) {
                exports.push(info);
            }
            // type declarations can be grouped but we only parse single-line here;
            // the grouped form is handled by the var/const path below using a shared helper
            continue;
        }
        if try_consume(bytes, &mut i, b"var") || try_consume(bytes, &mut i, b"const") {
            exports.extend(parse_var_const_decl(bytes, &mut i, line_start, original));
            continue;
        }

        // Not a keyword we care about — advance to next line.
        while i < len && bytes[i] != b'\n' {
            i += 1;
        }
    }

    exports
}

/// Attempt to consume `keyword` at `bytes[*i..]`, requiring that the next byte
/// after the keyword is not an identifier character (word boundary).
/// Advances `*i` past the keyword + whitespace on success; returns `true`.
fn try_consume(bytes: &[u8], i: &mut usize, keyword: &[u8]) -> bool {
    let end = *i + keyword.len();
    if end > bytes.len() {
        return false;
    }
    if &bytes[*i..end] != keyword {
        return false;
    }
    // Word boundary: next byte must not be alphanumeric or `_`.
    if end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        return false;
    }
    *i = end;
    skip_whitespace(bytes, i);
    true
}

/// Skip spaces and tabs (not newlines).
fn skip_whitespace(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && (bytes[*i] == b' ' || bytes[*i] == b'\t') {
        *i += 1;
    }
}

/// Parse an identifier starting at `bytes[*i]`.
/// Returns the identifier string and advances `*i`.
fn parse_ident<'a>(bytes: &'a [u8], i: &mut usize) -> Option<&'a str> {
    if *i >= bytes.len() {
        return None;
    }
    let b = bytes[*i];
    // Go identifiers start with a letter or `_`.
    if !b.is_ascii_alphabetic() && b != b'_' {
        // Allow multi-byte UTF-8 start bytes (≥0x80) for non-ASCII identifiers.
        if b < 0x80 {
            return None;
        }
    }
    let start = *i;
    while *i < bytes.len()
        && (bytes[*i].is_ascii_alphanumeric() || bytes[*i] == b'_' || bytes[*i] >= 0x80)
    {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    std::str::from_utf8(&bytes[start..*i]).ok()
}

/// Parse a `func` declaration.
/// On entry, `*i` points past `func` + whitespace.
/// Handles:
/// - Regular functions: `func Foo(...)`
/// - Generic functions: `func Foo[T any](...)`  (Go 1.18+)
/// - Methods with receivers: `func (r T) Foo(...)` — these are SKIPPED.
fn parse_func_decl(
    bytes: &[u8],
    i: &mut usize,
    decl_start: usize,
    original: &str,
    is_test_file: bool,
) -> Option<ExportInfo> {
    if *i >= bytes.len() {
        return None;
    }

    // If next byte is `(`, this is a method with a receiver — skip.
    if bytes[*i] == b'(' {
        skip_to_line_end(bytes, i);
        return None;
    }

    let name = parse_ident(bytes, i)?;

    // Determine visibility and always-live status.
    let (exported, visibility) = go_decl_visibility(name, is_test_file);

    // Only emit an ExportInfo if exported (uppercase) or always-live.
    if !exported && matches!(visibility, VisibilityTag::None) {
        skip_to_line_end(bytes, i);
        return None;
    }

    // If not actually uppercase (but still always-live), emit with visibility tag
    // but skip_to_line_end to avoid consuming brace depth incorrectly.
    if !exported {
        // init/main/TestXxx etc. — emit but mark as always-live.
        let span = byte_offset_span(decl_start, original);
        skip_to_line_end(bytes, i);
        return Some(ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: None,
            is_type_only: false,
            visibility,
            span,
            members: Vec::new(),
            super_class: None,
        });
    }

    let span = byte_offset_span(decl_start, original);
    skip_to_line_end(bytes, i);

    Some(ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: None,
        is_type_only: false,
        visibility,
        span,
        members: Vec::new(),
        super_class: None,
    })
}

/// Parse a `type` declaration.
/// On entry, `*i` points past `type` + whitespace.
fn parse_type_decl(
    bytes: &[u8],
    i: &mut usize,
    name_start: usize,
    original: &str,
) -> Option<ExportInfo> {
    let name = parse_ident(bytes, i)?;
    if !is_go_exported(name) {
        skip_to_line_end(bytes, i);
        return None;
    }
    let span = byte_offset_span(name_start, original);
    skip_to_line_end(bytes, i);

    // Type declarations may be interface or struct, but we record them uniformly.
    // Whether it's a type alias (`type Foo = bar.Baz`) is handled generically.
    Some(ExportInfo {
        name: ExportName::Named(name.to_string()),
        local_name: None,
        is_type_only: true,
        visibility: VisibilityTag::None,
        span,
        members: Vec::new(),
        super_class: None,
    })
}

/// Parse a `var` or `const` declaration (single or grouped).
/// On entry, `*i` points past the keyword + whitespace.
fn parse_var_const_decl(
    bytes: &[u8],
    i: &mut usize,
    _decl_start: usize,
    original: &str,
) -> Vec<ExportInfo> {
    let mut results = Vec::new();

    if *i >= bytes.len() {
        return results;
    }

    if bytes[*i] == b'(' {
        // Grouped: collect all exported identifiers inside the group.
        *i += 1; // consume `(`
        while *i < bytes.len() && bytes[*i] != b')' {
            skip_whitespace(bytes, i);
            if *i < bytes.len() && bytes[*i] == b'\n' {
                *i += 1;
                continue;
            }
            if *i < bytes.len() && bytes[*i] == b')' {
                break;
            }
            let name_start = *i;
            if let Some(name) = parse_ident(bytes, i)
                && is_go_exported(name)
            {
                let span = byte_offset_span(name_start, original);
                results.push(ExportInfo {
                    name: ExportName::Named(name.to_string()),
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span,
                    members: Vec::new(),
                    super_class: None,
                });
            }
            skip_to_line_end(bytes, i);
        }
        if *i < bytes.len() {
            *i += 1; // consume `)`
        }
    } else {
        // Single declaration.
        let name_start = *i;
        if let Some(name) = parse_ident(bytes, i)
            && is_go_exported(name)
        {
            let span = byte_offset_span(name_start, original);
            results.push(ExportInfo {
                name: ExportName::Named(name.to_string()),
                local_name: None,
                is_type_only: false,
                visibility: VisibilityTag::None,
                span,
                members: Vec::new(),
                super_class: None,
            });
        }
        skip_to_line_end(bytes, i);
    }

    results
}

/// Advance `*i` to the end of the current line (past the `\n`).
fn skip_to_line_end(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && bytes[*i] != b'\n' {
        *i += 1;
    }
    if *i < bytes.len() {
        *i += 1; // consume `\n`
    }
}

/// A Go identifier is exported if its first Unicode code point is an uppercase letter.
fn is_go_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Determine whether a Go identifier should be collected as an export and whether
/// it is always-live (init, main, test/bench/example/fuzz functions).
///
/// Returns `(is_exported_name, visibility_tag)`.
fn go_decl_visibility(name: &str, is_test_file: bool) -> (bool, VisibilityTag) {
    // Always-live: init() appears in any .go file, any number of times.
    if name == "init" || name == "main" {
        return (false, VisibilityTag::Public);
    }

    // Test/benchmark/example/fuzz functions in _test.go files.
    if is_test_file
        && (name.starts_with("Test")
            || name.starts_with("Benchmark")
            || name.starts_with("Example")
            || name.starts_with("Fuzz"))
    {
        return (is_go_exported(name), VisibilityTag::Public);
    }

    (is_go_exported(name), VisibilityTag::None)
}

/// Create a zero-length [`Span`] at the given byte offset, clamped to the
/// original source length. Used to anchor declaration positions.
#[expect(
    clippy::cast_possible_truncation,
    reason = "source files are practically < 4GB"
)]
fn byte_offset_span(offset: usize, source: &str) -> Span {
    let off = offset.min(source.len()) as u32;
    Span::new(off, off)
}

// ── Complexity extraction ────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct GoFunction {
    start: usize,
    body_start: usize,
    body_end: usize,
    params_start: usize,
    params_end: usize,
    name: Option<(usize, usize)>,
}

#[derive(Default, Clone, Copy)]
struct ComplexityMetrics {
    cyclomatic: u16,
    cognitive: u16,
}

fn extract_complexity(
    stripped: &str,
    line_offsets: &[u32],
) -> Vec<fallow_types::extract::FunctionComplexity> {
    let bytes = stripped.as_bytes();
    let mut results = Vec::new();
    scan_functions(bytes, 0, bytes.len(), line_offsets, &mut results);
    results
}

fn scan_functions(
    bytes: &[u8],
    start: usize,
    end: usize,
    line_offsets: &[u32],
    results: &mut Vec<fallow_types::extract::FunctionComplexity>,
) {
    let mut i = start;
    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        if is_keyword_at(bytes, i, b"func")
            && let Some(function) = parse_function_at(bytes, i, end)
        {
            let metrics = scan_region(bytes, function.body_start + 1, function.body_end, 0, false);
            let start_u32 = u32::try_from(function.start).unwrap_or(u32::MAX);
            let end_u32 = u32::try_from(function.body_end).unwrap_or(u32::MAX);
            let (line, col) =
                fallow_types::extract::byte_offset_to_line_col(line_offsets, start_u32);
            let end_line = fallow_types::extract::byte_offset_to_line_col(line_offsets, end_u32).0;
            let name = function
                .name
                .and_then(|(start, end)| std::str::from_utf8(&bytes[start..end]).ok())
                .unwrap_or("<anonymous>")
                .to_string();

            results.push(fallow_types::extract::FunctionComplexity {
                name,
                line,
                col,
                cyclomatic: metrics.cyclomatic.saturating_add(1),
                cognitive: metrics.cognitive,
                line_count: end_line.saturating_sub(line) + 1,
                param_count: count_go_params(bytes, function.params_start, function.params_end),
            });

            scan_functions(
                bytes,
                function.body_start + 1,
                function.body_end,
                line_offsets,
                results,
            );
            i = function.body_end.saturating_add(1);
            continue;
        }

        i += 1;
    }
}

fn scan_region(
    bytes: &[u8],
    start: usize,
    end: usize,
    nesting: u16,
    count_case_labels: bool,
) -> ComplexityMetrics {
    let mut metrics = ComplexityMetrics::default();
    let mut i = start;
    let mut last_logical_op: Option<u8> = None;

    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        if is_keyword_at(bytes, i, b"func")
            && let Some(function) = parse_function_at(bytes, i, end)
        {
            i = function.body_end.saturating_add(1);
            last_logical_op = None;
            continue;
        }

        if count_case_labels && is_keyword_at(bytes, i, b"case") {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
            i = skip_to_case_separator(bytes, i + 4, end);
            last_logical_op = None;
            continue;
        }
        if count_case_labels && is_keyword_at(bytes, i, b"default") {
            i = skip_to_case_separator(bytes, i + 7, end);
            last_logical_op = None;
            continue;
        }

        if is_keyword_at(bytes, i, b"if") {
            let parsed = parse_if_statement(bytes, i, end, nesting);
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(parsed.metrics.cyclomatic);
            metrics.cognitive = metrics.cognitive.saturating_add(parsed.metrics.cognitive);
            i = parsed.next;
            last_logical_op = None;
            continue;
        }

        if is_keyword_at(bytes, i, b"for")
            && let Some(parsed) = parse_block_statement(bytes, i, end, nesting, 3, false)
        {
            metrics.cyclomatic = metrics
                .cyclomatic
                .saturating_add(1 + parsed.metrics.cyclomatic);
            metrics.cognitive = metrics
                .cognitive
                .saturating_add(1 + nesting + parsed.metrics.cognitive);
            i = parsed.next;
            last_logical_op = None;
            continue;
        }

        if is_keyword_at(bytes, i, b"switch")
            && let Some(parsed) = parse_block_statement(bytes, i, end, nesting, 6, true)
        {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(parsed.metrics.cyclomatic);
            metrics.cognitive = metrics
                .cognitive
                .saturating_add(1 + nesting + parsed.metrics.cognitive);
            i = parsed.next;
            last_logical_op = None;
            continue;
        }

        if is_keyword_at(bytes, i, b"select")
            && let Some(parsed) = parse_block_statement(bytes, i, end, nesting, 6, true)
        {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(parsed.metrics.cyclomatic);
            metrics.cognitive = metrics
                .cognitive
                .saturating_add(1 + nesting + parsed.metrics.cognitive);
            i = parsed.next;
            last_logical_op = None;
            continue;
        }

        if is_keyword_at(bytes, i, b"else") {
            metrics.cognitive = metrics.cognitive.saturating_add(1);
            i += 4;
            last_logical_op = None;
            continue;
        }

        if i + 1 < end && bytes[i] == b'&' && bytes[i + 1] == b'&' {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
            if last_logical_op != Some(b'&') {
                metrics.cognitive = metrics.cognitive.saturating_add(1);
                last_logical_op = Some(b'&');
            }
            i += 2;
            continue;
        }
        if i + 1 < end && bytes[i] == b'|' && bytes[i + 1] == b'|' {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
            if last_logical_op != Some(b'|') {
                metrics.cognitive = metrics.cognitive.saturating_add(1);
                last_logical_op = Some(b'|');
            }
            i += 2;
            continue;
        }

        if matches!(bytes[i], b';' | b'{' | b'}' | b',') {
            last_logical_op = None;
        }

        if bytes[i] == b'{'
            && let Some(block_end) = find_matching(bytes, i, end, b'{', b'}')
        {
            let nested = scan_region(bytes, i + 1, block_end, nesting, false);
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(nested.cyclomatic);
            metrics.cognitive = metrics.cognitive.saturating_add(nested.cognitive);
            i = block_end.saturating_add(1);
            last_logical_op = None;
            continue;
        }

        i += 1;
    }

    metrics
}

fn scan_boolean_complexity(bytes: &[u8], start: usize, end: usize) -> ComplexityMetrics {
    let mut metrics = ComplexityMetrics::default();
    let mut i = start;
    let mut last_logical_op: Option<u8> = None;

    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        if i + 1 < end && bytes[i] == b'&' && bytes[i + 1] == b'&' {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
            if last_logical_op != Some(b'&') {
                metrics.cognitive = metrics.cognitive.saturating_add(1);
                last_logical_op = Some(b'&');
            }
            i += 2;
            continue;
        }
        if i + 1 < end && bytes[i] == b'|' && bytes[i + 1] == b'|' {
            metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
            if last_logical_op != Some(b'|') {
                metrics.cognitive = metrics.cognitive.saturating_add(1);
                last_logical_op = Some(b'|');
            }
            i += 2;
            continue;
        }

        if matches!(bytes[i], b';' | b'{' | b'}' | b',') {
            last_logical_op = None;
        }
        i += 1;
    }

    metrics
}

struct ParsedStatement {
    metrics: ComplexityMetrics,
    next: usize,
}

fn parse_if_statement(bytes: &[u8], start: usize, end: usize, nesting: u16) -> ParsedStatement {
    parse_if_statement_with_mode(bytes, start, end, nesting, false)
}

fn parse_if_statement_with_mode(
    bytes: &[u8],
    start: usize,
    end: usize,
    nesting: u16,
    else_if: bool,
) -> ParsedStatement {
    let mut metrics = ComplexityMetrics {
        cyclomatic: 1,
        cognitive: if else_if { 1 } else { 1 + nesting },
    };
    let body_start = find_statement_block_start(bytes, start + 2, end).unwrap_or(end);
    let body_end = find_matching(bytes, body_start, end, b'{', b'}').unwrap_or(body_start);
    let header = scan_boolean_complexity(bytes, start + 2, body_start);
    metrics.cyclomatic = metrics.cyclomatic.saturating_add(header.cyclomatic);
    metrics.cognitive = metrics.cognitive.saturating_add(header.cognitive);
    if body_start < end && body_end >= body_start {
        let nested = scan_region(
            bytes,
            body_start + 1,
            body_end,
            nesting.saturating_add(1),
            false,
        );
        metrics.cyclomatic = metrics.cyclomatic.saturating_add(nested.cyclomatic);
        metrics.cognitive = metrics.cognitive.saturating_add(nested.cognitive);
    }

    let mut next = body_end.saturating_add(1);
    next = skip_ascii_whitespace(bytes, next, end);
    if is_keyword_at(bytes, next, b"else") {
        let else_start = skip_ascii_whitespace(bytes, next + 4, end);
        if is_keyword_at(bytes, else_start, b"if") {
            let else_if_stmt = parse_if_statement_with_mode(bytes, else_start, end, nesting, true);
            metrics.cyclomatic = metrics
                .cyclomatic
                .saturating_add(else_if_stmt.metrics.cyclomatic);
            metrics.cognitive = metrics
                .cognitive
                .saturating_add(else_if_stmt.metrics.cognitive);
            next = else_if_stmt.next;
        } else if else_start < end && bytes[else_start] == b'{' {
            metrics.cognitive = metrics.cognitive.saturating_add(1);
            if let Some(else_end) = find_matching(bytes, else_start, end, b'{', b'}') {
                let nested = scan_region(
                    bytes,
                    else_start + 1,
                    else_end,
                    nesting.saturating_add(1),
                    false,
                );
                metrics.cyclomatic = metrics.cyclomatic.saturating_add(nested.cyclomatic);
                metrics.cognitive = metrics.cognitive.saturating_add(nested.cognitive);
                next = else_end.saturating_add(1);
            }
        } else {
            next = else_start;
        }
    }

    ParsedStatement { metrics, next }
}

fn parse_block_statement(
    bytes: &[u8],
    start: usize,
    end: usize,
    nesting: u16,
    keyword_len: usize,
    count_case_labels: bool,
) -> Option<ParsedStatement> {
    let body_start = find_statement_block_start(bytes, start + keyword_len, end)?;
    let body_end = find_matching(bytes, body_start, end, b'{', b'}')?;
    let mut metrics = scan_boolean_complexity(bytes, start + keyword_len, body_start);
    let nested = scan_region(
        bytes,
        body_start + 1,
        body_end,
        nesting.saturating_add(1),
        count_case_labels,
    );
    metrics.cyclomatic = metrics.cyclomatic.saturating_add(nested.cyclomatic);
    metrics.cognitive = metrics.cognitive.saturating_add(nested.cognitive);
    Some(ParsedStatement {
        metrics,
        next: body_end.saturating_add(1),
    })
}

fn parse_function_at(bytes: &[u8], start: usize, end: usize) -> Option<GoFunction> {
    if !is_keyword_at(bytes, start, b"func") {
        return None;
    }

    let mut i = skip_ascii_whitespace(bytes, start + 4, end);
    let mut name = None;
    let params_start;
    let params_end;

    if i < end && bytes[i] == b'(' {
        let first_group_end = find_matching(bytes, i, end, b'(', b')')?;
        let after_group = skip_ascii_whitespace(bytes, first_group_end + 1, end);
        if let Some(name_start) = parse_ident_start(bytes, after_group, end) {
            let mut name_end = name_start;
            parse_ident(bytes, &mut name_end)?;
            let mut maybe_params = skip_ascii_whitespace(bytes, name_end, end);
            if maybe_params < end && bytes[maybe_params] == b'[' {
                maybe_params =
                    find_matching(bytes, maybe_params, end, b'[', b']')?.saturating_add(1);
                maybe_params = skip_ascii_whitespace(bytes, maybe_params, end);
            }
            if maybe_params < end && bytes[maybe_params] == b'(' {
                name = Some((name_start, name_end));
                params_start = maybe_params;
                params_end = find_matching(bytes, params_start, end, b'(', b')')?;
            } else {
                params_start = i;
                params_end = first_group_end;
            }
        } else {
            params_start = i;
            params_end = first_group_end;
        }
    } else {
        let name_start = parse_ident_start(bytes, i, end)?;
        let mut name_end = name_start;
        parse_ident(bytes, &mut name_end)?;
        name = Some((name_start, name_end));
        i = skip_ascii_whitespace(bytes, name_end, end);
        if i < end && bytes[i] == b'[' {
            i = find_matching(bytes, i, end, b'[', b']')?.saturating_add(1);
            i = skip_ascii_whitespace(bytes, i, end);
        }
        params_start = i;
        params_end = find_matching(bytes, params_start, end, b'(', b')')?;
    }
    i = params_end.saturating_add(1);

    let body_start = find_statement_block_start(bytes, i, end)?;
    let body_end = find_matching(bytes, body_start, end, b'{', b'}')?;

    Some(GoFunction {
        start,
        body_start,
        body_end,
        params_start,
        params_end,
        name,
    })
}

fn find_statement_block_start(bytes: &[u8], mut i: usize, end: usize) -> Option<usize> {
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;

    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }

        match bytes[i] {
            b'(' => paren_depth = paren_depth.saturating_add(1),
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth = bracket_depth.saturating_add(1),
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => {
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                    return Some(i);
                }
                brace_depth = brace_depth.saturating_add(1);
            }
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }

    None
}

fn find_matching(bytes: &[u8], start: usize, end: usize, open: u8, close: u8) -> Option<usize> {
    if start >= end || bytes[start] != open {
        return None;
    }
    let mut depth = 0u32;
    let mut i = start;
    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            i = next;
            continue;
        }
        if bytes[i] == open {
            depth = depth.saturating_add(1);
        } else if bytes[i] == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn skip_string_or_rune(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    if start >= end {
        return None;
    }
    let quote = bytes[start];
    if !matches!(quote, b'"' | b'\'' | b'`') {
        return None;
    }

    let mut i = start + 1;
    while i < end {
        if quote != b'`' && bytes[i] == b'\\' {
            i = i.saturating_add(2);
            continue;
        }
        if bytes[i] == quote {
            return Some(i + 1);
        }
        i += 1;
    }
    Some(end)
}

fn is_keyword_at(bytes: &[u8], start: usize, keyword: &[u8]) -> bool {
    let end = start.saturating_add(keyword.len());
    if end > bytes.len() || &bytes[start..end] != keyword {
        return false;
    }
    let prev_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
    let next_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
    prev_ok && next_ok
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn parse_ident_start(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    if start >= end {
        return None;
    }
    let byte = bytes[start];
    if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 {
        Some(start)
    } else {
        None
    }
}

fn skip_ascii_whitespace(bytes: &[u8], mut start: usize, end: usize) -> usize {
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    start
}

fn skip_to_case_separator(bytes: &[u8], mut start: usize, end: usize) -> usize {
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;
    while start < end {
        if let Some(next) = skip_string_or_rune(bytes, start, end) {
            start = next;
            continue;
        }
        match bytes[start] {
            b'(' => paren_depth = paren_depth.saturating_add(1),
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth = bracket_depth.saturating_add(1),
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth = brace_depth.saturating_add(1),
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b':' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return start + 1;
            }
            _ => {}
        }
        start += 1;
    }
    end
}

fn count_go_params(bytes: &[u8], start: usize, end: usize) -> u8 {
    if start >= end || bytes[start] != b'(' || bytes[end] != b')' {
        return 0;
    }
    let mut count = 0u16;
    let mut has_token = false;
    let mut i = start + 1;
    while i < end {
        if let Some(next) = skip_string_or_rune(bytes, i, end) {
            if next > i + 1 {
                has_token = true;
            }
            i = next;
            continue;
        }
        match bytes[i] {
            b'(' => {
                i = find_matching(bytes, i, end, b'(', b')').unwrap_or(end);
                has_token = true;
            }
            b'[' => {
                i = find_matching(bytes, i, end, b'[', b']').unwrap_or(end);
                has_token = true;
            }
            b'{' => {
                i = find_matching(bytes, i, end, b'{', b'}').unwrap_or(end);
                has_token = true;
            }
            b',' => {
                count = count.saturating_add(1);
                has_token = false;
            }
            b if !b.is_ascii_whitespace() => has_token = true,
            _ => {}
        }
        i += 1;
    }
    if has_token {
        count = count.saturating_add(1);
    }
    u8::try_from(count).unwrap_or(u8::MAX)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::discover::FileId;
    use std::path::Path;

    fn parse(source: &str, name: &str) -> ModuleInfo {
        let path = Path::new(name);
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        parse_go_to_module(FileId(0), path, source, hash, false)
    }

    fn parse_with_complexity(source: &str, name: &str) -> ModuleInfo {
        let path = Path::new(name);
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        parse_go_to_module(FileId(0), path, source, hash, true)
    }

    fn parse_test(source: &str) -> ModuleInfo {
        parse(source, "foo_test.go")
    }

    // ── is_go_file ───────────────────────────────────────────────────

    #[test]
    fn detects_go_extension() {
        assert!(is_go_file(Path::new("main.go")));
        assert!(is_go_file(Path::new("/project/pkg/foo.go")));
    }

    #[test]
    fn rejects_non_go_extensions() {
        assert!(!is_go_file(Path::new("main.ts")));
        assert!(!is_go_file(Path::new("main.go.bak")));
        assert!(!is_go_file(Path::new("go")));
    }

    // ── Import extraction ────────────────────────────────────────────

    #[test]
    fn single_bare_import() {
        let src = r#"package main
import "fmt"
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports.len(), 1);
        assert_eq!(m.imports[0].source, "fmt");
        assert_eq!(m.imports[0].local_name, "fmt");
        assert!(matches!(
            m.imports[0].imported_name,
            ImportedName::Namespace
        ));
    }

    #[test]
    fn single_aliased_import() {
        let src = r#"package main
import m "math"
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports.len(), 1);
        assert_eq!(m.imports[0].source, "math");
        assert_eq!(m.imports[0].local_name, "m");
        assert!(matches!(
            m.imports[0].imported_name,
            ImportedName::Namespace
        ));
    }

    #[test]
    fn dot_import() {
        let src = r#"package main
import . "path/filepath"
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports.len(), 1);
        assert!(matches!(
            m.imports[0].imported_name,
            ImportedName::Namespace
        ));
    }

    #[test]
    fn blank_import() {
        let src = r#"package main
import _ "embed"
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports.len(), 1);
        assert!(matches!(
            m.imports[0].imported_name,
            ImportedName::SideEffect
        ));
    }

    #[test]
    fn grouped_imports() {
        let src = r#"package main
import (
    "fmt"
    "os"
    m "math"
    . "path/filepath"
    _ "embed"
)
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports.len(), 5);
        assert_eq!(m.imports[0].source, "fmt");
        assert_eq!(m.imports[2].local_name, "m");
        assert!(matches!(
            m.imports[3].imported_name,
            ImportedName::Namespace
        ));
        assert!(matches!(
            m.imports[4].imported_name,
            ImportedName::SideEffect
        ));
    }

    #[test]
    fn multi_segment_path_local_name() {
        let src = r#"package main
import "encoding/json"
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports[0].source, "encoding/json");
        assert_eq!(m.imports[0].local_name, "json");
    }

    #[test]
    fn selector_member_accesses_are_collected_for_imported_packages() {
        let src = r#"package main
import (
    "fmt"
    httpAlias "net/http"
)

func main() {
    fmt.Println(httpAlias.MethodGet)
}
"#;
        let m = parse(src, "main.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|a| a.object == "fmt" && a.member == "Println")
        );
        assert!(
            m.member_accesses
                .iter()
                .any(|a| a.object == "httpAlias" && a.member == "MethodGet")
        );
    }

    #[test]
    fn generic_method_expressions_are_collected_for_imported_packages() {
        let src = r#"package main
import shared "example.com/shared"

var _ = shared.Box[int].Run
"#;
        let m = parse(src, "main.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|a| a.object == "shared.Box" && a.member == "Run"),
            "member accesses: {:?}",
            m.member_accesses
        );
    }

    #[test]
    fn generic_method_expressions_are_collected_for_same_package_types() {
        let src = r#"package main

type Box[T any] struct{}

var _ = Box[int].Run
"#;
        let m = parse(src, "main.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|a| a.object == "Box" && a.member == "Run"),
            "member accesses: {:?}",
            m.member_accesses
        );
    }

    #[test]
    fn generic_exported_type_collects_receiver_methods() {
        let src = r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#;
        let m = parse(src, "main.go");
        let export = m
            .exports
            .iter()
            .find(|export| matches!(export.name, ExportName::Named(ref name) if name == "Box"))
            .expect("Box export");
        assert!(
            export.members.iter().any(|member| member.name == "Run"),
            "members: {:?}",
            export.members
        );
        assert!(
            export.members.iter().any(|member| member.name == "Stop"),
            "members: {:?}",
            export.members
        );
    }

    #[test]
    fn unexported_selector_members_are_ignored() {
        let src = r#"package main
import "example.com/foo"

func main() {
    foo.internalHelper()
    foo.Exported()
}
"#;
        let m = parse(src, "main.go");
        assert!(
            !m.member_accesses
                .iter()
                .any(|a| a.object == "foo" && a.member == "internalHelper")
        );
        assert!(
            m.member_accesses
                .iter()
                .any(|a| a.object == "foo" && a.member == "Exported")
        );
    }

    #[test]
    fn dot_import_member_accesses_are_collected_as_unqualified_symbols() {
        let src = r#"package main
import . "example.com/foo"

func main() {
    Exported()
}
"#;
        let m = parse(src, "main.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|a| a.object.is_empty() && a.member == "Exported")
        );
    }

    #[test]
    fn local_type_bindings_credit_same_file_receiver_methods() {
        let src = r#"package p

type Service struct{}

func (s Service) Run() {}

func Use() {
    svc := Service{}
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn imported_type_bindings_credit_cross_package_receiver_methods() {
        let src = r#"package p

import shared "github.com/acme/shared"

func Use() {
    svc := shared.Service{}
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_constructor_bindings_credit_receiver_methods() {
        let src = r#"package p

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := NewService()
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn imported_constructor_bindings_credit_receiver_methods() {
        let src = r#"package p

import shared "github.com/acme/shared"

func Use() {
    svc := shared.NewService()
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_alias_bindings_credit_receiver_methods() {
        let src = r#"package p

type Service struct{}

func (s Service) Run() {}

func Use() {
    svc := Service{}
    alias := svc
    alias.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn imported_alias_bindings_credit_receiver_methods() {
        let src = r#"package p

import shared "github.com/acme/shared"

func Use() {
    svc := shared.NewService()
    alias := svc
    alias.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_typed_var_prefers_constructor_target_over_interface_annotation() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    var svc Runner = NewService()
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn imported_typed_var_prefers_constructor_target_over_interface_annotation() {
        let src = r#"package p

import shared "github.com/acme/shared"

type Runner interface {
    Run()
}

func Use() {
    var svc Runner = shared.NewService()
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_helper_return_signature_credits_receiver_methods() {
        let src = r#"package p

type Service struct{}

func buildService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := buildService()
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_helper_return_prefers_concrete_value_over_interface_annotation() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func buildRunner() Runner { return NewService() }
func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := buildRunner()
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_multi_return_helper_with_consistent_targets_credits_receiver_methods() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func buildRunner(flag bool) Runner {
    if flag {
        return NewService()
    }
    return Service{}
}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := buildRunner(true)
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn local_multi_return_helper_with_conflicting_targets_stays_conservative() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}
type Other struct{}

func buildRunner(flag bool) Runner {
    if flag {
        return NewService()
    }
    return Other{}
}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (o Other) Run() {}

func Use() {
    svc := buildRunner(true)
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            !m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
        assert!(
            !m.member_accesses
                .iter()
                .any(|access| access.object == "Other" && access.member == "Run")
        );
    }

    #[test]
    fn forward_declared_local_helper_chain_credits_receiver_methods() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func buildRunner() Runner { return makeRunner() }

func Use() {
    svc := buildRunner()
    svc.Run()
}

func makeRunner() Runner { return NewService() }
func NewService() Service { return Service{} }
func (s Service) Run() {}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn single_param_passthrough_helper_credits_receiver_methods() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func wrapRunner(runner Runner) Runner { return runner }
func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := wrapRunner(NewService())
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn passthrough_helper_with_extra_logic_stays_conservative() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func wrapRunner(runner Runner) Runner {
    if runner == nil {
        return NewService()
    }
    return runner
}
func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := wrapRunner(NewService())
    svc.Run()
}
"#;
        let m = parse(src, "x.go");
        assert!(
            !m.member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_emits_local_helper_return_accesses() {
        let src = r#"package p

type Service struct{}

func buildService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := buildService()
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_emits_passthrough_helper_accesses() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func wrapRunner(runner Runner) Runner { return runner }
func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := wrapRunner(NewService())
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_stays_conservative_for_mixed_passthrough_logic() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func wrapRunner(runner Runner) Runner {
    if runner == nil {
        return NewService()
    }
    return runner
}
func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := wrapRunner(NewService())
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_merges_consistent_if_branch_bindings() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use(flag bool) {
    var svc Runner
    if flag {
        svc = NewService()
    } else {
        svc = Service{}
    }
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_stays_conservative_for_conflicting_if_branch_bindings() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}
type Other struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (o Other) Run() {}

func Use(flag bool) {
    var svc Runner
    if flag {
        svc = NewService()
    } else {
        svc = Other{}
    }
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Other" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_merges_consistent_switch_bindings() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use(mode int) {
    var svc Runner
    switch mode {
    case 1:
        svc = NewService()
    default:
        svc = Service{}
    }
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_stays_conservative_without_switch_default() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use(mode int) {
    var svc Runner
    switch mode {
    case 1:
        svc = NewService()
    }
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_resolves_helper_returns_through_local_bindings() {
        let src = r#"package p

type Service struct{}

func buildService() Service {
    svc := Service{}
    return svc
}

func (s Service) Run() {}

func Use() {
    svc := buildService()
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_resolves_passthrough_alias_returns() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func wrapRunner(runner Runner) Runner {
    alias := runner
    return alias
}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := wrapRunner(NewService())
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_routes_multi_param_helper_returns() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func chooseRunner(label string, runner Runner) Runner {
    alias := runner
    return alias
}

func NewService() Service { return Service{} }
func (s Service) Run() {}

func Use() {
    svc := chooseRunner("primary", NewService())
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_emits_type_checked_accesses_for_type_assertions() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}

func Use(r Runner) {
    svc := r.(Service)
    svc.Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
    }

    #[test]
    fn parser_helper_emits_type_checked_accesses_for_imported_type_assertions() {
        let src = r#"package p

import "bytes"

func Use(v any) {
    buf := v.(bytes.Buffer)
    buf.WriteString("x")
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "bytes.Buffer" && access.member == "WriteString")
        );
    }

    #[test]
    fn parser_helper_emits_type_checked_accesses_for_local_imported_type_assertions() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-parser-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(v any) {
    svc := v.(shared.Service)
    svc.Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_emits_type_checked_accesses_for_imported_type_switches() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-type-switch-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(v any) {
    switch svc := v.(type) {
    case shared.Service:
        svc.Run()
    }
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_emits_type_checked_accesses_for_imported_method_expressions() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-method-expr-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

var _ = shared.Service.Run
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_emits_interface_implementers_for_exported_types() {
        let src = r#"package p

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed.heritage.iter().any(|heritage| {
                heritage.export_name == "Service"
                    && heritage.implements == vec!["Runner".to_string()]
            }),
            "heritage: {:?}",
            parsed.heritage
        );
    }

    #[test]
    fn parser_helper_emits_type_checked_accesses_for_imported_interface_usage() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-interface-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(r shared.Runner) {
    r.Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_prefers_concrete_accesses_for_direct_helper_call_results() {
        let src = r#"package main

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}

func buildRunner() Runner {
    return Service{}
}

func main() {
    buildRunner().Run()
}
"#;
        let Some(parsed) = run_go_parser_helper(Path::new("x.go"), src) else {
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "Runner" && access.member == "Run"),
            "member accesses should prefer the concrete helper target: {:?}",
            parsed.member_accesses
        );
    }

    #[test]
    fn parser_helper_prefers_concrete_accesses_for_imported_helper_call_results() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-helper-call-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.BuildRunner().Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}

func BuildRunner() Runner {
    return Service{}
}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should prefer the concrete imported helper target: {:?}",
            parsed.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_local_imported_type_assertion_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(v any) {
    svc := v.(shared.Service)
    svc.Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
        assert_eq!(
            module.value_referenced_import_bindings,
            vec!["shared".to_string()]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_prefers_concrete_accesses_for_direct_helper_call_results() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-direct-helper-call-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}

func buildRunner() Runner {
    return Service{}
}

func main() {
    buildRunner().Run()
}
"#,
        )
        .expect("write main.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        assert!(
            !module
                .member_accesses
                .iter()
                .any(|access| access.object == "Runner" && access.member == "Run"),
            "member accesses should prefer the concrete helper target: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_prefers_concrete_accesses_for_imported_helper_call_results() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-helper-call-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.BuildRunner().Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}

func BuildRunner() Runner {
    return Service{}
}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        assert!(
            !module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should prefer the concrete imported helper target: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fallback_extracts_direct_same_package_composite_literal_accesses() {
        let stripped = r#"package main

func main() {
    Service{}.Run()
}
"#;
        let exports = vec![ExportInfo {
            name: ExportName::Named("Service".to_string()),
            local_name: Some("Service".to_string()),
            span: Span::new(0, 0),
            is_type_only: true,
            visibility: VisibilityTag::Public,
            members: Vec::new(),
            super_class: None,
        }];

        let accesses =
            extract_type_binding_member_accesses(Path::new("x.go"), stripped, &[], &exports, false);
        assert!(
            accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run"),
            "member accesses: {accesses:?}"
        );
    }

    #[test]
    fn fallback_extracts_direct_imported_composite_literal_accesses() {
        let stripped = r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.Service{}.Run()
}
"#;
        let imports = extract_imports(stripped);
        let accesses =
            extract_type_binding_member_accesses(Path::new("x.go"), stripped, &imports, &[], false);
        assert!(
            accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run"),
            "member accesses: {accesses:?}"
        );
    }

    #[test]
    fn fallback_extracts_parenthesized_addressed_imported_composite_literal_accesses() {
        let stripped = r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    (&shared.Service{}).Run()
}
"#;
        let imports = extract_imports(stripped);
        let accesses =
            extract_type_binding_member_accesses(Path::new("x.go"), stripped, &imports, &[], false);
        assert!(
            accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run"),
            "member accesses: {accesses:?}"
        );
    }

    #[test]
    fn fallback_extracts_direct_imported_generic_composite_literal_accesses() {
        let stripped = r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.Box[int]{}.Run()
}
"#;
        let imports = extract_imports(stripped);
        let accesses =
            extract_type_binding_member_accesses(Path::new("x.go"), stripped, &imports, &[], false);
        assert!(
            accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {accesses:?}"
        );
    }

    #[test]
    fn fallback_extracts_direct_imported_generic_helper_call_accesses() {
        let stripped = r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.NewBox[int]().Run()
}
"#;
        let imports = extract_imports(stripped);
        let accesses =
            extract_type_binding_member_accesses(Path::new("x.go"), stripped, &imports, &[], false);
        assert!(
            accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {accesses:?}"
        );
    }

    #[test]
    fn fallback_extracts_parenthesized_addressed_imported_generic_helper_call_accesses() {
        let stripped = r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    (&shared.NewBox[int]()).Run()
}
"#;
        let imports = extract_imports(stripped);
        let accesses =
            extract_type_binding_member_accesses(Path::new("x.go"), stripped, &imports, &[], false);
        assert!(
            accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {accesses:?}"
        );
    }

    #[test]
    fn parse_go_module_preserves_same_package_generic_method_expression_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-same-package-generic-method-expr-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

var _ = Box[int].Run

func main() {
    _ = Box[int]{}
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("box.go"),
            r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_same_package_generic_helper_call_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-same-package-generic-helper-call-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

func main() {
    NewBox[int]().Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("box.go"),
            r#"package main

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_prefers_concrete_accesses_for_imported_generic_typed_var_helper_results() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-generic-typed-var-helper-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func main() {
    var svc Runner = shared.NewBox[int]()
    svc.Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_prefers_concrete_accesses_for_generic_local_helper_interface_chains() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-generic-local-helper-chain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

type Runner interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

func buildRunner() Runner {
    return wrapRunner(NewBox[int]())
}

func wrapRunner(r Runner) Runner {
    return r
}

func main() {
    buildRunner().Run()
}
"#,
        )
        .expect("write main.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_prefers_concrete_accesses_for_imported_generic_helper_interface_chains() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-generic-helper-chain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return wrapRunner(shared.NewBox[int]())
}

func wrapRunner(r Runner) Runner {
    return r
}

func main() {
    buildRunner().Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_go_work_imported_generic_helper_interface_chains() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-work-generic-helper-chain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("app")).expect("create app");
        std::fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
        std::fs::write(
            root.join("go.work"),
            "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
        )
        .expect("write go.work");
        std::fs::write(
            root.join("app/go.mod"),
            "module github.com/acme/app\n\ngo 1.25\n",
        )
        .expect("write app/go.mod");
        std::fs::write(
            root.join("lib/go.mod"),
            "module github.com/acme/lib\n\ngo 1.25\n",
        )
        .expect("write lib/go.mod");
        std::fs::write(
            root.join("app/main.go"),
            r#"package main

import shared "github.com/acme/lib/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return wrapRunner(shared.NewBox[int]())
}

func wrapRunner(r Runner) Runner {
    return r
}

func main() {
    buildRunner().Run()
}
"#,
        )
        .expect("write app/main.go");
        std::fs::write(
            root.join("lib/pkg/shared/box.go"),
            r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write lib box.go");

        let source = std::fs::read_to_string(root.join("app/main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("app/main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_go_work_imported_generic_method_expressions() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-work-generic-method-expr-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("app")).expect("create app");
        std::fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
        std::fs::write(
            root.join("go.work"),
            "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
        )
        .expect("write go.work");
        std::fs::write(
            root.join("app/go.mod"),
            "module github.com/acme/app\n\ngo 1.25\n",
        )
        .expect("write app/go.mod");
        std::fs::write(
            root.join("lib/go.mod"),
            "module github.com/acme/lib\n\ngo 1.25\n",
        )
        .expect("write lib/go.mod");
        std::fs::write(
            root.join("app/main.go"),
            r#"package main

import shared "github.com/acme/lib/pkg/shared"

var _ = shared.Box[int].Run

func main() {
    _ = shared.Box[int]{}
}
"#,
        )
        .expect("write app/main.go");
        std::fs::write(
            root.join("lib/pkg/shared/box.go"),
            r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write lib box.go");

        let source = std::fs::read_to_string(root.join("app/main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("app/main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_go_work_imported_generic_type_assertion_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-work-generic-type-assert-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("app")).expect("create app");
        std::fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
        std::fs::write(
            root.join("go.work"),
            "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
        )
        .expect("write go.work");
        std::fs::write(
            root.join("app/go.mod"),
            "module github.com/acme/app\n\ngo 1.25\n",
        )
        .expect("write app/go.mod");
        std::fs::write(
            root.join("lib/go.mod"),
            "module github.com/acme/lib\n\ngo 1.25\n",
        )
        .expect("write lib/go.mod");
        std::fs::write(
            root.join("app/main.go"),
            r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(v any) {
    box := v.(shared.Box[int])
    box.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
        )
        .expect("write app/main.go");
        std::fs::write(
            root.join("lib/pkg/shared/box.go"),
            r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write lib box.go");

        let source = std::fs::read_to_string(root.join("app/main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("app/main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_go_work_imported_generic_type_switch_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-work-generic-type-switch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("app")).expect("create app");
        std::fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
        std::fs::write(
            root.join("go.work"),
            "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
        )
        .expect("write go.work");
        std::fs::write(
            root.join("app/go.mod"),
            "module github.com/acme/app\n\ngo 1.25\n",
        )
        .expect("write app/go.mod");
        std::fs::write(
            root.join("lib/go.mod"),
            "module github.com/acme/lib\n\ngo 1.25\n",
        )
        .expect("write lib/go.mod");
        std::fs::write(
            root.join("app/main.go"),
            r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(v any) {
    switch box := v.(type) {
    case shared.Box[int]:
        box.Run()
    }
}

func main() {
    use(shared.Box[int]{})
}
"#,
        )
        .expect("write app/main.go");
        std::fs::write(
            root.join("lib/pkg/shared/box.go"),
            r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write lib box.go");

        let source = std::fs::read_to_string(root.join("app/main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("app/main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_imported_generic_interface_usage_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-generic-interface-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(shared.Box[int]{})
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_go_work_imported_generic_interface_usage_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-work-generic-interface-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("app")).expect("create app");
        std::fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
        std::fs::write(
            root.join("go.work"),
            "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
        )
        .expect("write go.work");
        std::fs::write(
            root.join("app/go.mod"),
            "module github.com/acme/app\n\ngo 1.25\n",
        )
        .expect("write app/go.mod");
        std::fs::write(
            root.join("lib/go.mod"),
            "module github.com/acme/lib\n\ngo 1.25\n",
        )
        .expect("write lib/go.mod");
        std::fs::write(
            root.join("app/main.go"),
            r#"package main

import shared "github.com/acme/lib/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(shared.Box[int]{})
}
"#,
        )
        .expect("write app/main.go");
        std::fs::write(
            root.join("lib/pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write lib box.go");

        let source = std::fs::read_to_string(root.join("app/main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("app/main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_narrows_imported_generic_interface_usage_for_unexported_direct_calls() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-narrowed-generic-interface-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        assert!(
            !module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_narrows_imported_generic_interface_usage_for_unexported_direct_calls() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-parser-narrowed-generic-interface-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            panic!("parser helper should run");
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            parsed.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_narrows_imported_generic_interface_usage_for_unexported_helper_call_args() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-parser-narrowed-generic-helper-arg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    use(buildRunner())
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            panic!("parser helper should run");
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            parsed.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_narrows_imported_generic_interface_usage_for_unexported_helper_call_args() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-narrowed-generic-helper-arg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    use(buildRunner())
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        assert!(
            !module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_narrows_imported_generic_interface_usage_for_bound_helper_call_args() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-parser-narrowed-generic-bound-helper-arg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    svc := buildRunner()
    use(svc)
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            panic!("parser helper should run");
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            parsed.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_narrows_imported_generic_interface_usage_for_bound_helper_call_args() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-narrowed-generic-bound-helper-arg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    svc := buildRunner()
    use(svc)
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        assert!(
            !module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parser_helper_narrows_imported_generic_interface_usage_through_consistent_if_bindings() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-parser-narrowed-generic-if-bindings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    flag := true
    var svc shared.Runner[int]
    if flag {
        svc = shared.NewBox[int]()
    } else {
        svc = shared.Box[int]{}
    }
    use(svc)
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let Some(parsed) = run_go_parser_helper(&root.join("main.go"), &source) else {
            let _ = std::fs::remove_dir_all(&root);
            panic!("parser helper should run");
        };
        assert!(
            parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            parsed.member_accesses
        );
        assert!(
            !parsed
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            parsed.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_narrows_imported_generic_interface_usage_through_consistent_if_bindings() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-narrowed-generic-if-bindings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    flag := true
    var svc shared.Runner[int]
    if flag {
        svc = shared.NewBox[int]()
    } else {
        svc = shared.Box[int]{}
    }
    use(svc)
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/box.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
"#,
        )
        .expect("write box.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Box" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        assert!(
            !module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses should be narrowed away from the interface target: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_populates_interface_heritage() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-heritage-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("service.go"),
            r#"package main

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("service.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("service.go"), &source, hash, false);
        assert!(
            module.class_heritage.iter().any(|heritage| {
                heritage.export_name == "Service"
                    && heritage.implements == vec!["Runner".to_string()]
            }),
            "class heritage: {:?}",
            module.class_heritage
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_populates_imported_interface_heritage() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-heritage-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("service.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
        )
        .expect("write service.go");
        std::fs::write(
            root.join("pkg/shared/runner.go"),
            r#"package shared

type Runner interface {
    Run()
}
"#,
        )
        .expect("write runner.go");

        let source = std::fs::read_to_string(root.join("service.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("service.go"), &source, hash, false);
        assert!(
            module.class_heritage.iter().any(|heritage| {
                heritage.export_name == "Service"
                    && heritage.implements == vec!["shared.Runner".to_string()]
            }),
            "class heritage: {:?}",
            module.class_heritage
        );
        let service_export = module
            .exports
            .iter()
            .find(|export| matches!(export.name, ExportName::Named(ref name) if name == "Service"))
            .expect("service export");
        assert!(
            service_export
                .members
                .iter()
                .any(|member| member.name == "Run"),
            "service members: {:?}",
            service_export.members
        );
        assert!(
            service_export
                .members
                .iter()
                .any(|member| member.name == "Stop"),
            "service members: {:?}",
            service_export.members
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_populates_imported_generic_interface_heritage() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-generic-heritage-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("box.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
        )
        .expect("write box.go");
        std::fs::write(
            root.join("pkg/shared/runner.go"),
            r#"package shared

type Runner[T any] interface {
    Run()
}
"#,
        )
        .expect("write runner.go");

        let source = std::fs::read_to_string(root.join("box.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("box.go"), &source, hash, false);
        assert!(
            module.class_heritage.iter().any(|heritage| {
                heritage.export_name == "Box"
                    && heritage.implements == vec!["shared.Runner".to_string()]
            }),
            "class heritage: {:?}",
            module.class_heritage
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_imported_interface_usage_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-imported-interface-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
        std::fs::write(
            root.join("main.go"),
            r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(r shared.Runner) {
    r.Run()
}
"#,
        )
        .expect("write main.go");
        std::fs::write(
            root.join("pkg/shared/service.go"),
            r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");

        let source = std::fs::read_to_string(root.join("main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Runner" && access.member == "Run"),
            "member accesses: {:?}",
            module.member_accesses
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_same_package_type_assertion_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-same-package-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("go.mod"),
            "module github.com/acme/example\n\ngo 1.25\n",
        )
        .expect("write go.mod");
        std::fs::write(
            root.join("service.go"),
            r#"package main

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write service.go");
        std::fs::write(
            root.join("use.go"),
            r#"package main

func Use(v Runner) {
    svc := v.(Service)
    svc.Run()
}
"#,
        )
        .expect("write use.go");

        let source = std::fs::read_to_string(root.join("use.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("use.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "Service" && access.member == "Run")
        );
        let service_source =
            std::fs::read_to_string(root.join("service.go")).expect("read service source");
        let service_hash = xxhash_rust::xxh3::xxh3_64(service_source.as_bytes());
        let service_module = parse_go_to_module(
            FileId(1),
            &root.join("service.go"),
            &service_source,
            service_hash,
            false,
        );
        let service_export = service_module
            .exports
            .iter()
            .find(|export| matches!(export.name, ExportName::Named(ref name) if name == "Service"))
            .expect("service export");
        assert!(
            service_export
                .members
                .iter()
                .any(|member| member.name == "Run"),
            "service members: {:?}",
            service_export.members
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_go_module_preserves_go_work_imported_type_assertion_accesses() {
        let root = std::env::temp_dir().join(format!(
            "fallow-go-work-parse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("app")).expect("create app dir");
        std::fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib dir");
        std::fs::write(
            root.join("go.work"),
            "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
        )
        .expect("write go.work");
        std::fs::write(
            root.join("app/go.mod"),
            "module github.com/acme/app\n\ngo 1.25\n",
        )
        .expect("write app/go.mod");
        std::fs::write(
            root.join("lib/go.mod"),
            "module github.com/acme/lib\n\ngo 1.25\n",
        )
        .expect("write lib/go.mod");
        std::fs::write(
            root.join("app/main.go"),
            r#"package main

import shared "github.com/acme/lib/pkg/shared"

func Use(v any) {
    svc := v.(shared.Service)
    svc.Run()
}
"#,
        )
        .expect("write app/main.go");
        std::fs::write(
            root.join("lib/pkg/shared/service.go"),
            r#"package shared

type Service struct{}

func (s Service) Run() {}
"#,
        )
        .expect("write shared service");

        let source = std::fs::read_to_string(root.join("app/main.go")).expect("read source");
        let hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
        let module = parse_go_to_module(FileId(0), &root.join("app/main.go"), &source, hash, false);
        assert!(
            module
                .member_accesses
                .iter()
                .any(|access| access.object == "shared.Service" && access.member == "Run")
        );
        assert_eq!(
            module.value_referenced_import_bindings,
            vec!["shared".to_string()]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn go_import_binding_usage_treats_qualified_type_targets_as_used_packages() {
        let imports = vec![ImportInfo {
            source: "github.com/acme/example/pkg/shared".to_string(),
            imported_name: ImportedName::Namespace,
            local_name: "shared".to_string(),
            is_type_only: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        }];
        let member_accesses = vec![MemberAccess {
            object: "shared.Service".to_string(),
            member: "Run".to_string(),
        }];

        let usage = compute_go_import_binding_usage(&imports, &member_accesses);
        assert_eq!(usage.value_referenced, vec!["shared".to_string()]);
        assert!(usage.unused.is_empty());
    }

    #[test]
    fn go_import_binding_usage_tracks_used_and_unused_packages() {
        let src = r#"package main
import (
    "fmt"
    httpAlias "net/http"
    "os"
    _ "embed"
    . "path/filepath"
)

func main() {
    fmt.Println(httpAlias.MethodGet)
}
"#;
        let m = parse(src, "main.go");
        assert_eq!(
            m.value_referenced_import_bindings,
            vec!["fmt".to_string(), "httpAlias".to_string()]
        );
        assert_eq!(m.unused_import_bindings, vec!["os".to_string()]);
    }

    // ── Export extraction ────────────────────────────────────────────

    #[test]
    fn exported_function() {
        let src = r#"package pkg
func Exported() {}
func unexported() {}
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].name, ExportName::Named("Exported".to_string()));
    }

    #[test]
    fn method_receiver_skipped() {
        let src = r#"package pkg
func (r *Recv) Method() {}
func Standalone() {}
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(
            m.exports[0].name,
            ExportName::Named("Standalone".to_string())
        );
    }

    #[test]
    fn exported_type() {
        let src = r#"package pkg
type MyType struct{}
type unexportedType struct{}
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].name, ExportName::Named("MyType".to_string()));
        assert!(m.exports[0].is_type_only);
    }

    #[test]
    fn exported_type_collects_receiver_methods_and_fields() {
        let src = r#"package pkg
type Service struct {
    Name string
    hidden string
}

func (s Service) Run() {}
func (s *Service) Stop() {}
func (s Service) hidden() {}
"#;
        let m = parse(src, "pkg.go");
        let service = m
            .exports
            .iter()
            .find(|export| export.name == ExportName::Named("Service".to_string()))
            .expect("Service export");
        assert!(
            service.members.iter().any(|member| {
                member.name == "Name" && member.kind == MemberKind::ClassProperty
            })
        );
        assert!(
            service
                .members
                .iter()
                .any(|member| { member.name == "Run" && member.kind == MemberKind::ClassMethod })
        );
        assert!(
            service
                .members
                .iter()
                .any(|member| { member.name == "Stop" && member.kind == MemberKind::ClassMethod })
        );
        assert!(!service.members.iter().any(|member| member.name == "hidden"));
    }

    #[test]
    fn exported_var() {
        let src = r#"package pkg
var ErrNotFound = errors.New("not found")
var internal = 42
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(
            m.exports[0].name,
            ExportName::Named("ErrNotFound".to_string())
        );
    }

    #[test]
    fn exported_const_grouped() {
        let src = r#"package pkg
const (
    ExportedA = 1
    ExportedB = 2
    unexported = 3
)
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 2);
    }

    #[test]
    fn generic_function_go125() {
        let src = r#"package pkg
func Map[T, U any](s []T, f func(T) U) []U { return nil }
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].name, ExportName::Named("Map".to_string()));
    }

    // ── Always-live markers ──────────────────────────────────────────

    #[test]
    fn init_always_live() {
        let src = r#"package pkg
func init() { setup() }
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].visibility, VisibilityTag::Public);
    }

    #[test]
    fn main_always_live() {
        let src = r#"package main
func main() {}
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].visibility, VisibilityTag::Public);
    }

    #[test]
    fn test_functions_live_in_test_file() {
        let src = r#"package foo
func TestFoo(t *testing.T) {}
func BenchmarkFoo(b *testing.B) {}
func ExampleFoo() {}
func FuzzFoo(f *testing.F) {}
"#;
        let m = parse_test(src);
        assert_eq!(m.exports.len(), 4);
        for e in &m.exports {
            assert_eq!(e.visibility, VisibilityTag::Public);
        }
    }

    #[test]
    fn test_functions_not_special_in_non_test_file() {
        let src = r#"package foo
func TestFoo(t *testing.T) {}
"#;
        let m = parse(src, "foo.go");
        // TestFoo starts with uppercase so it IS exported, but not always-live.
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].visibility, VisibilityTag::None);
    }

    #[test]
    fn comment_stripping_ignores_import_in_comment() {
        let src = r#"package main
// import "fake"
/* import "also_fake" */
import "real"
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.imports.len(), 1);
        assert_eq!(m.imports[0].source, "real");
    }

    #[test]
    fn generic_type_alias_go125() {
        let src = r#"package pkg
type Result[T any] = other.Result[T]
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.exports.len(), 1);
        assert_eq!(m.exports[0].name, ExportName::Named("Result".to_string()));
    }

    #[test]
    fn go_source_parses_suppressions() {
        let src = r#"// fallow-ignore-next-line unused-export
package pkg

// fallow-ignore-file unused-file
func Exported() {}
"#;
        let m = parse(src, "pkg.go");
        assert_eq!(m.suppressions.len(), 2);
        assert_eq!(
            m.suppressions[0].kind,
            Some(crate::suppress::IssueKind::UnusedExport)
        );
        assert_eq!(
            m.suppressions[1].kind,
            Some(crate::suppress::IssueKind::UnusedFile)
        );
    }

    #[test]
    fn go_complexity_counts_control_flow_and_boolean_sequences() {
        let src = r#"package pkg

func Complex(a, b, c bool, items []int) int {
    if a && b || c {
        for _, item := range items {
            switch item {
            case 1:
                return item
            case 2:
                return item
            default:
                return 0
            }
        }
    } else {
        return 0
    }

    return 1
}
"#;
        let m = parse_with_complexity(src, "pkg.go");
        let f = m.complexity.iter().find(|c| c.name == "Complex").unwrap();
        assert_eq!(f.cyclomatic, 7);
        assert_eq!(f.cognitive, 9);
        assert_eq!(f.param_count, 4);
    }

    #[test]
    fn go_complexity_handles_methods_and_nested_func_literals_independently() {
        let src = r#"package pkg

type Worker struct{}

func (w Worker) Run(flag bool) {
    inner := func(enabled bool) int {
        if enabled {
            return 1
        }
        return 0
    }

    if flag {
        _ = inner(flag)
    }
}
"#;
        let m = parse_with_complexity(src, "pkg.go");
        let run = m.complexity.iter().find(|c| c.name == "Run").unwrap();
        let inner = m
            .complexity
            .iter()
            .find(|c| c.name == "<anonymous>")
            .unwrap();
        assert_eq!(run.cyclomatic, 2);
        assert_eq!(run.cognitive, 1);
        assert_eq!(run.param_count, 1);
        assert_eq!(inner.cyclomatic, 2);
        assert_eq!(inner.cognitive, 1);
        assert_eq!(inner.param_count, 1);
    }

    #[test]
    fn go_flags_detect_os_env_guard() {
        let src = r#"package main
import "os"

func main() {
    if os.Getenv("FEATURE_NEW_CHECKOUT") != "" {
        run()
    }
}
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.flag_uses.len(), 1);
        assert_eq!(m.flag_uses[0].flag_name, "FEATURE_NEW_CHECKOUT");
        assert_eq!(m.flag_uses[0].kind, FlagUseKind::EnvVar);
        assert!(m.flag_uses[0].guard_span_start.is_some());
    }

    #[test]
    fn go_flags_detect_sdk_calls() {
        let src = r#"package main

func enabled(client LDClient) bool {
    return client.BoolVariation("beta-search", nil, false)
}
"#;
        let m = parse(src, "main.go");
        assert_eq!(m.flag_uses.len(), 1);
        assert_eq!(m.flag_uses[0].flag_name, "beta-search");
        assert_eq!(m.flag_uses[0].kind, FlagUseKind::SdkCall);
        assert_eq!(m.flag_uses[0].sdk_name.as_deref(), Some("LaunchDarkly"));
    }

    #[test]
    fn go_flags_ignore_non_flag_env_vars() {
        let src = r#"package main
import "os"

func main() {
    _ = os.Getenv("DATABASE_URL")
}
"#;
        let m = parse(src, "main.go");
        assert!(m.flag_uses.is_empty());
    }

    #[test]
    fn go_flags_support_custom_prefixes_and_sdk_patterns() {
        let src = r#"package main
import "os"

func main() {
    _ = os.Getenv("MYAPP_ENABLE_V2")
    _ = IsFeatureActive("rollout-a")
}
"#;
        let flags = extract_go_flags_from_source(
            src,
            Path::new("main.go"),
            &[("IsFeatureActive".to_string(), 0, "Internal".to_string())],
            &["MYAPP_ENABLE_".to_string()],
        );
        assert_eq!(flags.len(), 2);
        assert!(flags.iter().any(|flag| {
            flag.flag_name == "MYAPP_ENABLE_V2" && flag.kind == FlagUseKind::EnvVar
        }));
        assert!(flags.iter().any(|flag| {
            flag.flag_name == "rollout-a"
                && flag.kind == FlagUseKind::SdkCall
                && flag.sdk_name.as_deref() == Some("Internal")
        }));
    }

    #[test]
    fn go_public_signature_refs_capture_private_types() {
        let src = r#"package pkg

type privateOptions struct{}
type exportedBacking struct{}

func Build(opts privateOptions) exportedBacking { return exportedBacking{} }

type Service struct {
    opts privateOptions
}
"#;
        let m = parse(src, "pkg.go");
        assert!(
            m.local_type_declarations
                .iter()
                .any(|decl| decl.name == "privateOptions")
        );
        assert!(m.public_signature_type_references.iter().any(|reference| {
            reference.export_name == "Build" && reference.type_name == "privateOptions"
        }));
        assert!(m.public_signature_type_references.iter().any(|reference| {
            reference.export_name == "Service" && reference.type_name == "privateOptions"
        }));
        assert!(m.public_signature_type_references.iter().any(|reference| {
            reference.export_name == "Build" && reference.type_name == "exportedBacking"
        }));
    }
}
