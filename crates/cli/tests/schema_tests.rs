#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use std::fs;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;

#[test]
fn schema_outputs_valid_json() {
    let output = run_fallow_raw(&["schema"]);
    assert_eq!(output.code, 0, "schema should exit 0");
    let json = parse_json(&output);
    assert!(json.is_object(), "schema output should be a JSON object");
}

#[test]
fn schema_has_name_and_version() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    assert_eq!(
        json["name"].as_str().unwrap(),
        "fallow",
        "schema name should be 'fallow'"
    );
    assert!(
        json.get("version").is_some(),
        "schema should have version field"
    );
}

#[test]
fn schema_has_commands_array() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    let commands = json["commands"].as_array().unwrap();
    assert!(!commands.is_empty(), "schema should list commands");

    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"audit"), "should list audit command");
    assert!(
        names.contains(&"dead-code"),
        "should list dead-code command"
    );
    assert!(names.contains(&"health"), "should list health command");
    assert!(names.contains(&"dupes"), "should list dupes command");
    assert!(names.contains(&"inspect"), "should list inspect command");
    assert!(names.contains(&"explain"), "should list explain command");
}

#[test]
fn explain_outputs_rule_guidance_as_json() {
    let output = run_fallow_raw(&["explain", "unused-exports", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("fallow/unused-export"));
    assert!(json["example"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(json["how_to_fix"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn explain_accepts_issue_labels_with_spaces() {
    let output = run_fallow_raw(&["explain", "unused", "files", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("fallow/unused-file"));
}

#[test]
fn explain_compact_is_single_line() {
    let output = run_fallow_raw(&[
        "explain",
        "unused-exports",
        "--format",
        "compact",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    assert_eq!(
        output.stdout.trim(),
        "explain:fallow/unused-export:Export is never imported:https://docs.fallow.tools/explanations/dead-code#unused-exports"
    );
}

#[test]
fn explain_markdown_is_markdown() {
    let output = run_fallow_raw(&[
        "explain",
        "unused-exports",
        "--format",
        "markdown",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    assert!(output.stdout.starts_with("# Unused Exports\n\n"));
    assert!(output.stdout.contains("## Why it matters"));
    assert!(
        output
            .stdout
            .contains("[Docs](https://docs.fallow.tools/explanations/dead-code#unused-exports)")
    );
}

#[test]
fn explain_rejects_unknown_issue_type() {
    let output = run_fallow_raw(&["explain", "not-a-real-rule", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 2, "unknown explain id should exit 2");
    let json = parse_json(&output);
    assert_eq!(json["error"].as_bool(), Some(true));
}

#[test]
fn explain_outputs_tainted_sink_guidance_as_json() {
    let output = run_fallow_raw(&["explain", "tainted-sink", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("security/tainted-sink"));
    assert!(
        json["rationale"]
            .as_str()
            .is_some_and(|s| s.contains("unverified candidates"))
    );
    assert!(
        json["example"]
            .as_str()
            .is_some_and(|s| s.contains("security/sql-injection"))
    );
}

#[test]
fn explain_outputs_client_server_leak_guidance_as_json() {
    let output = run_fallow_raw(&[
        "explain",
        "client-server-leak",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("security/client-server-leak"));
    assert!(
        json["rationale"]
            .as_str()
            .is_some_and(|s| s.contains("process.env"))
    );
    assert!(
        json["example"]
            .as_str()
            .is_some_and(|s| s.contains("use client"))
    );
}

#[test]
fn explain_outputs_hardcoded_secret_guidance_as_json() {
    let output = run_fallow_raw(&["explain", "hardcoded-secret", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("security/hardcoded-secret"));
    assert!(
        json["rationale"]
            .as_str()
            .is_some_and(|s| s.contains("include-required"))
    );
    assert!(
        json["how_to_fix"]
            .as_str()
            .is_some_and(|s| s.contains("Rotate real credentials"))
    );
}

#[test]
fn explain_accepts_security_catalogue_ids() {
    let output = run_fallow_raw(&["explain", "sql-injection", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("security/sql-injection"));
    assert_eq!(json["name"].as_str(), Some("SQL injection sink"));
    assert!(
        json["rationale"]
            .as_str()
            .is_some_and(|s| s.contains("CWE-89"))
    );

    let output = run_fallow_raw(&["explain", "security/ssrf", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "explain should exit 0: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["id"].as_str(), Some("security/ssrf"));
    assert_eq!(
        json["name"].as_str(),
        Some("Server-side request forgery sink")
    );
}

#[test]
fn explain_security_unknown_suggests_security_examples() {
    let output = run_fallow_raw(&[
        "explain",
        "security-not-a-real-rule",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 2, "unknown explain id should exit 2");
    let json = parse_json(&output);
    let message = json["message"]
        .as_str()
        .or_else(|| json["error_message"].as_str())
        .unwrap_or("");
    assert!(message.contains("tainted-sink"), "message was: {message}");
    assert!(
        message.contains("client-server-leak"),
        "message was: {message}"
    );
    assert!(
        message.contains("hardcoded-secret"),
        "message was: {message}"
    );
}

#[test]
fn schema_has_issue_types() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    let types = json["issue_types"].as_array().unwrap();
    assert!(!types.is_empty(), "schema should list issue types");
}

#[test]
fn schema_has_exit_codes() {
    let output = run_fallow_raw(&["schema"]);
    let json = parse_json(&output);
    assert!(
        json.get("exit_codes").is_some(),
        "schema should document exit codes"
    );
}

#[test]
fn json_schema_vec_and_option_skip_fields_have_serde_default() {
    let root = workspace_root();
    let mut offenders = Vec::new();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);

    for file in files {
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", file.display()));
        collect_schema_default_offenders(&root, &file, &syntax.items, &mut offenders);
    }

    assert!(
        offenders.is_empty(),
        "JsonSchema Vec<T>/Option<T> fields using skip_serializing_if must include serde(default).\n{}",
        offenders.join("\n")
    );
}

#[test]
fn json_schema_default_gate_reports_missing_default_with_fix_hint() {
    let source = r#"#[derive(schemars::JsonSchema)]
struct Example {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    items: Vec<String>,
}"#;
    let syntax = syn::parse_file(source).expect("synthetic schema struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_schema_default_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(offenders.len(), 1);
    assert_eq!(
        offenders[0],
        "crates/fake/src/lib.rs:4: Example.items uses #[serde(skip_serializing_if = \"Vec::is_empty\")] without default; use #[serde(default, skip_serializing_if = \"Vec::is_empty\")]"
    );
}

#[test]
fn json_schema_default_gate_walks_enum_struct_variants() {
    let source = r#"#[derive(schemars::JsonSchema)]
enum Example {
    Variant {
        #[serde(skip_serializing_if = "Vec::is_empty")]
        items: Vec<String>,
    },
}"#;
    let syntax = syn::parse_file(source).expect("synthetic schema enum should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_schema_default_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(offenders.len(), 1);
    assert_eq!(
        offenders[0],
        "crates/fake/src/lib.rs:5: Example::Variant.items uses #[serde(skip_serializing_if = \"Vec::is_empty\")] without default; use #[serde(default, skip_serializing_if = \"Vec::is_empty\")]"
    );
}

#[test]
fn json_schema_default_gate_accepts_default_in_any_serde_position() {
    let source = r#"#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct Example {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    maybe: Option<String>,
}"#;
    let syntax = syn::parse_file(source).expect("synthetic schema struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_schema_default_offenders(root, &file, &syntax.items, &mut offenders);

    assert!(offenders.is_empty());
}

#[test]
fn path_fields_use_serde_path_serializer() {
    let root = workspace_root();
    let mut offenders = Vec::new();

    for source_root in [
        root.join("crates/types/src"),
        root.join("crates/core/src"),
        root.join("crates/cli/src"),
    ] {
        let mut files = Vec::new();
        collect_rs_files(&source_root, &mut files);

        for file in files {
            let source = fs::read_to_string(&file)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
            let syntax = syn::parse_file(&source)
                .unwrap_or_else(|err| panic!("failed to parse {}: {err}", file.display()));
            collect_path_field_offenders(&root, &file, &syntax.items, &mut offenders);
        }
    }

    assert!(
        offenders.is_empty(),
        "Serialize-deriving PathBuf fields must use serde_path serializers for cross-platform JSON paths.\n{}",
        offenders.join("\n")
    );
}

#[test]
fn path_field_gate_reports_missing_scalar_option_and_vec_serializers() {
    let source = r"#[derive(Serialize)]
struct Example {
    file: PathBuf,
    maybe: Option<PathBuf>,
    files: Vec<PathBuf>,
}";
    let syntax = syn::parse_file(source).expect("synthetic path struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_path_field_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(
        offenders,
        vec![
            "crates/fake/src/lib.rs:3: Example.file is PathBuf and derives Serialize without a path serializer; use #[serde(serialize_with = \"serde_path::serialize\")]",
            "crates/fake/src/lib.rs:4: Example.maybe is Option<PathBuf> and derives Serialize without a path serializer; use #[serde(serialize_with = \"serde_path::serialize_option\")]",
            "crates/fake/src/lib.rs:5: Example.files is Vec<PathBuf> and derives Serialize without a path serializer; use #[serde(serialize_with = \"serde_path::serialize_vec\")]",
        ]
    );
}

#[test]
fn path_field_gate_handles_fully_qualified_types() {
    let source = r"#[derive(serde::Serialize)]
struct Example {
    file: std::path::PathBuf,
    maybe: std::option::Option<std::path::PathBuf>,
    files: std::vec::Vec<std::path::PathBuf>,
}";
    let syntax = syn::parse_file(source).expect("synthetic path struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_path_field_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(offenders.len(), 3);
    assert!(offenders[0].contains("Example.file is PathBuf"));
    assert!(offenders[1].contains("Example.maybe is Option<PathBuf>"));
    assert!(offenders[2].contains("Example.files is Vec<PathBuf>"));
}

#[test]
fn path_field_gate_accepts_cfg_attr_serialize_derives() {
    let source = r#"#[cfg_attr(feature = "schema", derive(Debug, serde::Serialize))]
struct Example {
    file: PathBuf,
}"#;
    let syntax = syn::parse_file(source).expect("synthetic path struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_path_field_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(offenders.len(), 1);
    assert!(offenders[0].contains("Example.file is PathBuf"));
}

#[test]
fn path_field_gate_walks_and_skips_enum_struct_variants() {
    let source = r"#[derive(Serialize)]
enum Example {
    Visible {
        file: PathBuf,
    },
    #[serde(skip)]
    Hidden {
        file: PathBuf,
    },
}";
    let syntax = syn::parse_file(source).expect("synthetic path enum should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_path_field_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(offenders.len(), 1);
    assert!(offenders[0].contains("Example::Visible.file is PathBuf"));
}

#[test]
fn path_field_gate_accepts_skipped_fields_and_custom_scalar_option_serializers() {
    let source = r#"#[derive(Serialize)]
struct Example {
    #[serde(skip)]
    skipped: PathBuf,
    #[serde(skip_serializing)]
    skipped_serializing: PathBuf,
    #[serde(serialize_with = "custom_path")]
    file: PathBuf,
    #[serde(serialize_with = "custom_option_path")]
    maybe: Option<PathBuf>,
}"#;
    let syntax = syn::parse_file(source).expect("synthetic path struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_path_field_offenders(root, &file, &syntax.items, &mut offenders);

    assert!(offenders.is_empty());
}

#[test]
fn path_field_gate_requires_serialize_vec_for_vec_paths() {
    let source = r#"#[derive(Serialize)]
struct Example {
    #[serde(serialize_with = "custom_vec")]
    custom: Vec<PathBuf>,
    #[serde(serialize_with = "serde_path::serialize_vec")]
    local: Vec<PathBuf>,
    #[serde(serialize_with = "crate::serde_path::serialize_vec")]
    crate_path: Vec<PathBuf>,
    #[serde(serialize_with = "fallow_types::serde_path::serialize_vec")]
    external_path: Vec<PathBuf>,
}"#;
    let syntax = syn::parse_file(source).expect("synthetic path struct should parse");
    let root = Path::new("/workspace");
    let file = root.join("crates/fake/src/lib.rs");
    let mut offenders = Vec::new();

    collect_path_field_offenders(root, &file, &syntax.items, &mut offenders);

    assert_eq!(offenders.len(), 1);
    assert!(offenders[0].contains("Example.custom is Vec<PathBuf>"));
    assert!(offenders[0].contains("serde_path::serialize_vec"));
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli should have a workspace parent")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to read {}: {err}", dir.display()))
    {
        let entry = entry.unwrap_or_else(|err| panic!("failed to read dir entry: {err}"));
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn collect_schema_default_offenders(
    root: &Path,
    file: &Path,
    items: &[syn::Item],
    offenders: &mut Vec<String>,
) {
    for item in items {
        match item {
            syn::Item::Struct(item_struct) if derives_json_schema(&item_struct.attrs) => {
                collect_fields_offenders(
                    root,
                    file,
                    &item_struct.fields,
                    &item_struct.ident.to_string(),
                    offenders,
                );
            }
            syn::Item::Enum(item_enum) if derives_json_schema(&item_enum.attrs) => {
                for variant in &item_enum.variants {
                    if matches!(variant.fields, syn::Fields::Named(_)) {
                        let owner = format!("{}::{}", item_enum.ident, variant.ident);
                        collect_fields_offenders(root, file, &variant.fields, &owner, offenders);
                    }
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    collect_schema_default_offenders(root, file, nested_items, offenders);
                }
            }
            _ => {}
        }
    }
}

fn collect_path_field_offenders(
    root: &Path,
    file: &Path,
    items: &[syn::Item],
    offenders: &mut Vec<String>,
) {
    for item in items {
        match item {
            syn::Item::Struct(item_struct) if derives_serialize(&item_struct.attrs) => {
                collect_path_fields_offenders(
                    root,
                    file,
                    &item_struct.fields,
                    &item_struct.ident.to_string(),
                    offenders,
                );
            }
            syn::Item::Enum(item_enum) if derives_serialize(&item_enum.attrs) => {
                for variant in &item_enum.variants {
                    if matches!(variant.fields, syn::Fields::Named(_))
                        && !serde_skips_serialization(&variant.attrs)
                    {
                        let owner = format!("{}::{}", item_enum.ident, variant.ident);
                        collect_path_fields_offenders(
                            root,
                            file,
                            &variant.fields,
                            &owner,
                            offenders,
                        );
                    }
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    collect_path_field_offenders(root, file, nested_items, offenders);
                }
            }
            _ => {}
        }
    }
}

fn collect_path_fields_offenders(
    root: &Path,
    file: &Path,
    fields: &syn::Fields,
    owner: &str,
    offenders: &mut Vec<String>,
) {
    for (index, field) in fields.iter().enumerate() {
        let Some(kind) = path_field_kind(&field.ty) else {
            continue;
        };
        if serde_skips_serialization(&field.attrs)
            || path_field_has_valid_serializer(&field.attrs, kind)
        {
            continue;
        }

        let field_name = field
            .ident
            .as_ref()
            .map_or_else(|| index.to_string(), ToString::to_string);
        let relative = file.strip_prefix(root).unwrap_or(file);
        let line = field.ident.as_ref().map_or_else(
            || field.span().start().line,
            |ident| ident.span().start().line,
        );
        offenders.push(format!(
            "{}:{line}: {owner}.{field_name} is {} and derives Serialize without a path serializer; use {}",
            relative.display(),
            kind.label(),
            kind.fix_hint()
        ));
    }
}

#[derive(Clone, Copy)]
enum PathFieldKind {
    Scalar,
    Option,
    Vec,
}

impl PathFieldKind {
    fn label(self) -> &'static str {
        match self {
            Self::Scalar => "PathBuf",
            Self::Option => "Option<PathBuf>",
            Self::Vec => "Vec<PathBuf>",
        }
    }

    fn fix_hint(self) -> &'static str {
        match self {
            Self::Scalar => "#[serde(serialize_with = \"serde_path::serialize\")]",
            Self::Option => "#[serde(serialize_with = \"serde_path::serialize_option\")]",
            Self::Vec => "#[serde(serialize_with = \"serde_path::serialize_vec\")]",
        }
    }
}

fn path_field_kind(ty: &syn::Type) -> Option<PathFieldKind> {
    if is_pathbuf_type(ty) {
        return Some(PathFieldKind::Scalar);
    }

    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    let inner = first_angle_bracket_type(segment)?;
    if !is_pathbuf_type(inner) {
        return None;
    }
    if segment.ident == "Option" {
        Some(PathFieldKind::Option)
    } else if segment.ident == "Vec" {
        Some(PathFieldKind::Vec)
    } else {
        None
    }
}

fn is_pathbuf_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "PathBuf")
}

fn first_angle_bracket_type(segment: &syn::PathSegment) -> Option<&syn::Type> {
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

fn path_field_has_valid_serializer(attrs: &[syn::Attribute], kind: PathFieldKind) -> bool {
    let Some(serializer) = serde_serialize_with(attrs) else {
        return false;
    };
    match kind {
        PathFieldKind::Scalar | PathFieldKind::Option => true,
        PathFieldKind::Vec => serializer
            .rsplit("::")
            .next()
            .is_some_and(|segment| segment == "serialize_vec"),
    }
}

fn derives_serialize(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive") && attr_tokens_contain(attr, "Serialize")
            || attr.path().is_ident("cfg_attr") && attr_tokens_contain(attr, "Serialize")
    })
}

fn collect_fields_offenders(
    root: &Path,
    file: &Path,
    fields: &syn::Fields,
    owner: &str,
    offenders: &mut Vec<String>,
) {
    for (index, field) in fields.iter().enumerate() {
        let Some(kind) = vec_or_option_kind(&field.ty) else {
            continue;
        };
        let skip_fn = match kind {
            FieldKind::Vec => "Vec::is_empty",
            FieldKind::Option => "Option::is_none",
        };
        if !serde_has_skip_serializing_if(&field.attrs, skip_fn) || serde_has_default(&field.attrs)
        {
            continue;
        }

        let field_name = field
            .ident
            .as_ref()
            .map_or_else(|| index.to_string(), ToString::to_string);
        let relative = file.strip_prefix(root).unwrap_or(file);
        let line = field.ident.as_ref().map_or_else(
            || field.span().start().line,
            |ident| ident.span().start().line,
        );
        offenders.push(format!(
            "{}:{line}: {owner}.{field_name} uses #[serde(skip_serializing_if = \"{skip_fn}\")] without default; use #[serde(default, skip_serializing_if = \"{skip_fn}\")]",
            relative.display(),
        ));
    }
}

#[derive(Clone, Copy)]
enum FieldKind {
    Vec,
    Option,
}

fn vec_or_option_kind(ty: &syn::Type) -> Option<FieldKind> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let ident = &type_path.path.segments.last()?.ident;
    if ident == "Vec" {
        Some(FieldKind::Vec)
    } else if ident == "Option" {
        Some(FieldKind::Option)
    } else {
        None
    }
}

fn derives_json_schema(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive") && attr_tokens_contain(attr, "JsonSchema")
            || attr.path().is_ident("cfg_attr") && attr_tokens_contain(attr, "JsonSchema")
    })
}

fn attr_tokens_contain(attr: &syn::Attribute, needle: &str) -> bool {
    match &attr.meta {
        syn::Meta::List(list) => list.tokens.to_string().contains(needle),
        _ => false,
    }
}

fn serde_has_default(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("serde") {
            return false;
        }
        let mut has_default = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                has_default = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _value: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        });
        has_default
    })
}

fn serde_skips_serialization(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("serde") {
            return false;
        }
        let mut skips = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                skips = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _value: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        });
        skips
    })
}

fn serde_serialize_with(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("serde") {
            return None;
        }
        let mut serializer = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("serialize_with") {
                serializer = Some(meta.value()?.parse::<syn::LitStr>()?.value());
            }
            Ok(())
        });
        serializer
    })
}

fn serde_has_skip_serializing_if(attrs: &[syn::Attribute], expected: &str) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("serde") {
            return false;
        }
        let mut has_skip = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip_serializing_if") {
                let value = meta.value()?.parse::<syn::LitStr>()?.value();
                has_skip = value == expected;
            }
            Ok(())
        });
        has_skip
    })
}

#[test]
fn config_schema_outputs_valid_json() {
    let output = run_fallow_raw(&["config-schema"]);
    assert_eq!(output.code, 0, "config-schema should exit 0");
    let json = parse_json(&output);
    assert!(json.is_object(), "config-schema should be a JSON object");
}

#[test]
fn config_schema_is_json_schema() {
    let output = run_fallow_raw(&["config-schema"]);
    let json = parse_json(&output);
    assert!(
        json.get("$schema").is_some() || json.get("type").is_some(),
        "config-schema should be a JSON Schema document"
    );
}

#[test]
fn plugin_schema_outputs_valid_json() {
    let output = run_fallow_raw(&["plugin-schema"]);
    assert_eq!(output.code, 0, "plugin-schema should exit 0");
    let json = parse_json(&output);
    assert!(json.is_object(), "plugin-schema should be a JSON object");
}

#[test]
fn plugin_schema_is_json_schema() {
    let output = run_fallow_raw(&["plugin-schema"]);
    let json = parse_json(&output);
    assert!(
        json.get("$schema").is_some() || json.get("type").is_some(),
        "plugin-schema should be a JSON Schema document"
    );
}
