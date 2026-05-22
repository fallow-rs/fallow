#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use std::fs;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;

// ---------------------------------------------------------------------------
// schema command
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// config-schema command
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// plugin-schema command
// ---------------------------------------------------------------------------

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
