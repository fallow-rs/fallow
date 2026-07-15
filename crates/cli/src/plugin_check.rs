//! `fallow plugin-check`: a read-only dry-run that surfaces, as structured JSON,
//! what each external plugin actually does, so an agent authoring a
//! `fallow-plugin-*.jsonc` can verify it (write -> run -> observe -> fix)
//! without parsing stderr `tracing::warn!` noise.
//!
//! For every external plugin in the resolved config it reports whether the
//! plugin ACTIVATED (the agent's #1 failure is a plugin whose detection does
//! not match, which was previously silent), and for active plugins with
//! `manifestEntries` the per-rule report from `check_manifest_entries`.

use std::path::Path;
use std::process::ExitCode;

use colored::Colorize;
use fallow_config::{ExternalPluginDef, OutputFormat, PackageJson, PluginDetection};
use serde_json::{Value, json};

use fallow_engine::plugins::{
    CheckWarning, RuleReport, WarningKind, check_manifest_entries, is_external_plugin_active,
};

use crate::error::emit_error;

/// Caveat surfaced at the JSON root: `path_exists` is a filesystem check, not a
/// reachability proof (the command deliberately skips the discovery walk to stay
/// fast on large repos).
const NOTE: &str = "path_exists reports whether a file matches the seeded glob on disk. \
    false means the entry is definitely broken; true is necessary but NOT sufficient \
    (it does not prove the entry became reachable, since discovery still filters \
    gitignored and wrong-extension files).";

/// Run the `plugin-check` command.
pub fn run_plugin_check(
    root: &Path,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let doc = match build_plugin_check_doc(root) {
        Ok(doc) => doc,
        Err(e) => return emit_error(&e, 2, output),
    };
    if matches!(output, OutputFormat::Human) {
        print_human(&doc);
        return ExitCode::SUCCESS;
    }
    match render_plugin_check_json(&doc, json_style) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => crate::error::emit_error_with_style(
            &format!("failed to serialize plugin-check: {e}"),
            2,
            output,
            json_style,
        ),
    }
}

fn render_plugin_check_json(
    doc: &Value,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, serde_json::Error> {
    json_style.serialize(doc)
}

/// Build the `plugin-check` JSON document (pure, no output side effects).
fn build_plugin_check_doc(root: &Path) -> Result<Value, String> {
    // Canonicalize so a `..`-relative root does not make every seeded entry fail
    // the under-root check (main.rs already canonicalizes `--root`, but a
    // programmatic caller might not).
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let root = canonical.as_path();
    let project = fallow_engine::project_config::config_for_project(root, None)
        .map_err(|e| format!("failed to load config: {e}"))?;
    let external = &project.config.external_plugins;

    // Activation needs the project's dependency set; the manifest walk and
    // detection's file-existence fallback do the filesystem work, so an empty
    // discovered-file set is passed (no full source-discovery walk).
    let info = crate::init::detect_project(root);
    let root_pkg = PackageJson::load(&root.join("package.json")).ok();
    let all_deps = crate::init::collect_dependency_names(root, root_pkg.as_ref(), info.is_monorepo);

    let mut plugins: Vec<Value> = external
        .iter()
        .map(|ext| plugin_value(ext, &all_deps, root))
        .collect();
    plugins.sort_by(|a, b| plugin_name(a).cmp(plugin_name(b)));

    Ok(json!({
        "kind": "plugin-check",
        "schema_version": "1",
        "note": NOTE,
        "plugins": plugins,
    }))
}

fn plugin_name(plugin: &Value) -> &str {
    plugin.get("name").and_then(Value::as_str).unwrap_or("")
}

/// Build the JSON for one external plugin.
fn plugin_value(ext: &ExternalPluginDef, all_deps: &[String], root: &Path) -> Value {
    let active = is_external_plugin_active(ext, all_deps, root, &[]);
    if !active {
        return json!({
            "name": ext.name,
            "active": false,
            "activation_requirement": activation_requirement(ext),
        });
    }
    let rules: Vec<Value> = check_manifest_entries(ext, root)
        .iter()
        .map(|report| rule_value(report, root))
        .collect();
    json!({
        "name": ext.name,
        "active": true,
        "manifest_rules": rules,
    })
}

/// Build the JSON for one `manifestEntries` rule report, enriching each seeded
/// entry with `path_exists` and appending the check-only `seeded-paths-missing`
/// warning when a rule seeded entries but none exist on disk.
fn rule_value(report: &RuleReport, root: &Path) -> Value {
    let mut any_seeded = false;
    let mut any_exists = false;
    let matched: Vec<Value> = report
        .matched
        .iter()
        .map(|manifest| {
            let seeded: Vec<Value> = manifest
                .seeded
                .iter()
                .map(|entry| {
                    let exists = entry_path_exists(root, entry);
                    any_seeded = true;
                    any_exists |= exists;
                    json!({ "path": entry, "path_exists": exists })
                })
                .collect();
            json!({
                "path": manifest.path,
                "when_passed": manifest.when_passed,
                "seeded": seeded,
            })
        })
        .collect();

    let mut warnings: Vec<Value> = report.warnings.iter().map(warning_value).collect();
    if any_seeded && !any_exists {
        warnings.push(warning_value(&CheckWarning {
            kind: WarningKind::SeededPathsMissing,
            glob: Some(report.manifests.clone()),
            field_path: None,
            manifest: None,
            entry: None,
        }));
    }
    warnings.sort_by(|a, b| warning_kind(a).cmp(warning_kind(b)));

    json!({
        "manifests": report.manifests,
        "manifests_matched": report.manifests_matched,
        "matched": matched,
        "warnings": warnings,
    })
}

fn warning_kind(warning: &Value) -> &str {
    warning.get("kind").and_then(Value::as_str).unwrap_or("")
}

/// Serialize a warning as `{ kind, <typed slot> }` (agents read the slot their
/// `kind` implies rather than parsing prose).
fn warning_value(warning: &CheckWarning) -> Value {
    let mut value = json!({ "kind": warning.kind.as_kebab() });
    if let Some(glob) = &warning.glob {
        value["glob"] = json!(glob);
    }
    if let Some(field_path) = &warning.field_path {
        value["field_path"] = json!(field_path);
    }
    if let Some(manifest) = &warning.manifest {
        value["manifest"] = json!(manifest);
    }
    if let Some(entry) = &warning.entry {
        value["entry"] = json!(entry);
    }
    value
}

/// Whether a seeded entry glob (`public/index.{ts,tsx}`) matches any file on
/// disk. Brace groups are expanded first (the `glob` crate does not support
/// them); wildcard expansions are checked via `glob::glob`, literal ones via a
/// direct stat.
fn entry_path_exists(root: &Path, entry: &str) -> bool {
    expand_braces(entry).iter().any(|pattern| {
        let full = root.join(pattern);
        if pattern.contains(['*', '?', '[']) {
            glob::glob(&full.to_string_lossy())
                .ok()
                .is_some_and(|mut paths| paths.next().is_some())
        } else {
            full.exists()
        }
    })
}

/// Expand `{a,b}` brace groups into their cartesian product. `index.{ts,tsx}`
/// yields `index.ts`, `index.tsx`; a pattern with no braces yields itself.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_offset) = pattern[open..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + close_offset;
    let prefix = &pattern[..open];
    let options = &pattern[open + 1..close];
    let suffix = &pattern[close + 1..];
    let tails = expand_braces(suffix);
    let mut out = Vec::new();
    for option in options.split(',') {
        for tail in &tails {
            out.push(format!("{prefix}{option}{tail}"));
        }
    }
    out
}

/// A human-readable description of why an inactive plugin did not activate.
fn activation_requirement(ext: &ExternalPluginDef) -> String {
    if let Some(detection) = &ext.detection {
        format!(
            "detection not met: requires {}",
            describe_detection(detection)
        )
    } else if !ext.enablers.is_empty() {
        format!(
            "no enabler present; requires one of these dependencies: {}",
            ext.enablers.join(", ")
        )
    } else {
        "no detection or enablers declared, so the plugin can never activate".to_string()
    }
}

fn describe_detection(detection: &PluginDetection) -> String {
    match detection {
        PluginDetection::Dependency { package } => format!("dependency '{package}'"),
        PluginDetection::FileExists { pattern } => format!("a file matching '{pattern}'"),
        PluginDetection::All { conditions } => format!(
            "all of [{}]",
            conditions
                .iter()
                .map(describe_detection)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        PluginDetection::Any { conditions } => format!(
            "any of [{}]",
            conditions
                .iter()
                .map(describe_detection)
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
}

/// Human output: a per-plugin summary with a loud warning-count line so a
/// warning is impossible to miss on the exit-0 advisory path.
fn print_human(doc: &Value) {
    let plugins = doc.get("plugins").and_then(Value::as_array);
    let Some(plugins) = plugins else {
        return;
    };
    if plugins.is_empty() {
        println!("No external plugins found in the resolved config.");
        return;
    }
    let mut total_warnings = 0usize;
    for plugin in plugins {
        let name = plugin_name(plugin);
        let active = plugin
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !active {
            let reason = plugin
                .get("activation_requirement")
                .and_then(Value::as_str)
                .unwrap_or("");
            println!("plugin '{name}': INACTIVE ({reason})");
            continue;
        }
        let rules = plugin.get("manifest_rules").and_then(Value::as_array);
        let rules = rules.map_or(&[][..], |r| r.as_slice());
        if rules.is_empty() {
            println!("plugin '{name}': active, 0 manifestEntries rules");
            continue;
        }
        for rule in rules {
            let glob = rule.get("manifests").and_then(Value::as_str).unwrap_or("");
            let matched = rule
                .get("manifests_matched")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let seeded: usize = rule
                .get("matched")
                .and_then(Value::as_array)
                .map_or(0, |ms| {
                    ms.iter()
                        .filter_map(|m| m.get("seeded").and_then(Value::as_array))
                        .map(Vec::len)
                        .sum()
                });
            let warnings = rule.get("warnings").and_then(Value::as_array);
            let warn_count = warnings.map_or(0, Vec::len);
            total_warnings += warn_count;
            println!(
                "plugin '{name}': rule '{glob}' matched {matched} manifest(s), seeded {seeded} \
                 entry pattern(s), {warn_count} warning(s)"
            );
            if let Some(warnings) = warnings {
                for warning in warnings {
                    println!("    ! {}", human_warning(warning));
                }
            }
        }
    }
    if total_warnings == 0 {
        // Colored for the TTY human (impossible-to-miss pass/fail); `colored`
        // auto-disables on non-TTY / NO_COLOR, so the agent-pipe stays plain.
        println!("\n{}", "0 warnings.".green());
    } else {
        let plural = if total_warnings == 1 { "" } else { "s" };
        println!(
            "\n{}",
            format!(
                "{total_warnings} warning{plural} above. Fix them so your plugin seeds what you \
                 expect."
            )
            .yellow()
        );
    }
}

/// A one-line human remediation for a warning.
fn human_warning(warning: &Value) -> String {
    let kind = warning_kind(warning);
    let slot = |key: &str| warning.get(key).and_then(Value::as_str).unwrap_or("");
    match kind {
        "manifests-matched-none" => format!(
            "manifests-matched-none: glob '{}' matched no files. Check the glob and whether the \
             manifests are under an ignored directory.",
            slot("glob")
        ),
        "when-excluded-all" => format!(
            "when-excluded-all: the 'when' gate excluded every manifest matched by '{}'. \
             Loosen the gate or check the field values.",
            slot("glob")
        ),
        "field-path-unresolved" => format!(
            "field-path-unresolved: field path '{}' resolved in no matched manifest. Likely a \
             typo in a 'when' key or a ${{...}} interpolation.",
            slot("field_path")
        ),
        "entries-empty" => format!(
            "entries-empty: rule for '{}' has an empty 'entries' list; it seeds nothing.",
            slot("glob")
        ),
        "manifest-parse-failed" => format!(
            "manifest-parse-failed: manifest '{}' could not be read or parsed (check it is valid \
             JSON/JSONC).",
            slot("manifest")
        ),
        "entry-outside-root" => format!(
            "entry-outside-root: entry '{}' (from '{}') resolved outside the project root and was \
             skipped.",
            slot("entry"),
            slot("manifest")
        ),
        "seeded-paths-missing" => format!(
            "seeded-paths-missing: rule for '{}' seeded entries but none exist on disk. The \
             ${{...}} interpolation likely resolved to the wrong directory.",
            slot("glob")
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_braces_single_group() {
        assert_eq!(
            expand_braces("public/index.{ts,tsx}"),
            vec!["public/index.ts", "public/index.tsx"]
        );
    }

    #[test]
    fn expand_braces_no_group_passthrough() {
        assert_eq!(expand_braces("public/index.ts"), vec!["public/index.ts"]);
    }

    #[test]
    fn expand_braces_multiple_groups_cartesian() {
        assert_eq!(
            expand_braces("{a,b}/index.{ts,tsx}"),
            vec![
                "a/index.ts".to_string(),
                "a/index.tsx".to_string(),
                "b/index.ts".to_string(),
                "b/index.tsx".to_string(),
            ]
        );
    }

    #[test]
    fn entry_path_exists_literal_and_brace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("public")).unwrap();
        std::fs::write(root.join("public/index.tsx"), "export {}").unwrap();
        // brace glob resolves to index.tsx which exists
        assert!(entry_path_exists(root, "public/index.{ts,tsx}"));
        // a literal that does not exist
        assert!(!entry_path_exists(root, "server/index.ts"));
    }

    #[test]
    fn every_warning_kind_has_a_human_remediation() {
        // Exhaustive match: adding a WarningKind is a compile error here, forcing
        // the new kind into the list below (and thus a human_warning arm), so a
        // future kind cannot silently degrade to the bare kebab fallback.
        fn all_kinds() -> Vec<WarningKind> {
            fn _exhaustive(k: WarningKind) {
                match k {
                    WarningKind::ManifestsMatchedNone
                    | WarningKind::WhenExcludedAll
                    | WarningKind::FieldPathUnresolved
                    | WarningKind::EntriesEmpty
                    | WarningKind::ManifestParseFailed
                    | WarningKind::EntryOutsideRoot
                    | WarningKind::SeededPathsMissing => {}
                }
            }
            vec![
                WarningKind::ManifestsMatchedNone,
                WarningKind::WhenExcludedAll,
                WarningKind::FieldPathUnresolved,
                WarningKind::EntriesEmpty,
                WarningKind::ManifestParseFailed,
                WarningKind::EntryOutsideRoot,
                WarningKind::SeededPathsMissing,
            ]
        }

        for kind in all_kinds() {
            let warning = warning_value(&CheckWarning {
                kind,
                glob: Some("g".into()),
                field_path: Some("f".into()),
                manifest: Some("m".into()),
                entry: Some("e".into()),
            });
            let message = human_warning(&warning);
            assert!(
                message.len() > kind.as_kebab().len() + 8,
                "WarningKind '{}' has no human remediation (falls back to the bare kind)",
                kind.as_kebab()
            );
        }
    }

    #[test]
    fn warning_value_carries_only_the_relevant_slot() {
        let w = warning_value(&CheckWarning {
            kind: WarningKind::FieldPathUnresolved,
            glob: None,
            field_path: Some("plugin.typo".to_string()),
            manifest: None,
            entry: None,
        });
        assert_eq!(w["kind"], "field-path-unresolved");
        assert_eq!(w["field_path"], "plugin.typo");
        assert!(w.get("glob").is_none());
    }

    #[test]
    fn plugin_check_json_respects_explicit_style() {
        let doc = json!({"kind": "plugin-check", "plugins": []});
        let compact = render_plugin_check_json(&doc, crate::json_style::JsonStyle::Compact)
            .expect("compact plugin-check should serialize");
        let pretty = render_plugin_check_json(&doc, crate::json_style::JsonStyle::Pretty)
            .expect("pretty plugin-check should serialize");

        assert!(
            !compact.contains('\n'),
            "compact JSON must stay on one line"
        );
        assert!(pretty.contains("\n  \""), "pretty JSON must be indented");
        assert_eq!(
            serde_json::from_str::<Value>(&compact).unwrap(),
            serde_json::from_str::<Value>(&pretty).unwrap(),
        );
    }

    fn kibana_fixture() -> std::path::PathBuf {
        // Canonicalize to mirror how `main.rs` resolves `--root` before dispatch
        // (a `..`-relative root would make every seeded entry fail the
        // under-root check and spuriously warn `entry-outside-root`).
        std::fs::canonicalize(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/manifest-entries-kibana"),
        )
        .unwrap()
    }

    #[test]
    fn doc_reports_active_kibana_plugin_seeded_entries() {
        let doc = build_plugin_check_doc(&kibana_fixture()).expect("build doc");
        assert_eq!(doc["kind"], "plugin-check");
        let plugins = doc["plugins"].as_array().unwrap();
        let kibana = plugins.iter().find(|p| p["name"] == "kibana").unwrap();
        assert_eq!(kibana["active"], true);
        let rule = &kibana["manifest_rules"][0];
        assert_eq!(rule["warnings"].as_array().unwrap().len(), 0);

        let beta = rule["matched"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["path"] == "plugins/beta/kibana.jsonc")
            .unwrap();
        // beta has server:false, so only public is seeded.
        let seeded: Vec<&str> = beta["seeded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["path"].as_str().unwrap())
            .collect();
        assert!(seeded.iter().any(|s| s.contains("beta/public/index")));
        assert!(
            !seeded.iter().any(|s| s.contains("server/index")),
            "beta server:false must not seed the server entry, got {seeded:?}"
        );
    }

    #[test]
    fn doc_reports_inactive_plugin_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"name":"x"}"#).unwrap();
        std::fs::write(
            root.join("fallow-plugin-x.jsonc"),
            r#"{
                "name": "x",
                "detection": { "type": "fileExists", "pattern": "**/never-exists.config.js" },
                "entryPoints": ["src/index.ts"]
            }"#,
        )
        .unwrap();

        let doc = build_plugin_check_doc(root).expect("build doc");
        let plugins = doc["plugins"].as_array().unwrap();
        let plugin = plugins.iter().find(|p| p["name"] == "x").unwrap();
        assert_eq!(
            plugin["active"], false,
            "plugin with an unmet detection must report active:false, not be omitted"
        );
        assert!(
            plugin["activation_requirement"]
                .as_str()
                .unwrap()
                .contains("never-exists"),
            "activation_requirement should name the unmet detection, got {:?}",
            plugin["activation_requirement"]
        );
    }
}
