//! Evaluation of external-plugin `manifestEntries` rules.
//!
//! A [`ManifestEntryRule`] seeds entry points DERIVED from framework manifest
//! files: it finds manifests by a recursive glob (a bounded, `.gitignore`-aware
//! second walk, because manifests are config files and are NOT in the
//! source-discovery set), parses each one, and for every manifest that passes
//! the rule-level `when` gate resolves each `entries[].path` relative to that
//! manifest's directory (with `${dotted.field}` interpolation) into a
//! root-relative entry pattern.
//!
//! The dominant failure mode is silent-none across a large manifest set (a typo
//! in a field path seeds nothing), so evaluation emits loud `tracing::warn!`
//! diagnostics: a `manifests` glob that matches nothing, a `when` that excludes
//! every matched manifest, a referenced field path that resolves in zero
//! matched manifests, an empty `entries` list, and unparseable manifests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fallow_config::{ExternalPluginDef, ManifestEntryRule};
use serde_json::Value;

use super::PathRule;
use super::config_parser::normalize_config_path;

/// Evaluate every `manifestEntries` rule on an active external plugin, returning
/// the root-relative entry patterns to seed (each is a glob matched literally
/// against discovered files, so it must encode its own extension).
///
/// Manifest files are config files, not source files, so they are not in the
/// source-discovery set; this does a bounded `.gitignore`-respecting walk (like
/// plugin detection's file-existence fallback) to find them. Manifests under
/// gitignored / `node_modules` directories are intentionally invisible.
#[must_use]
pub fn evaluate_manifest_entries(ext: &ExternalPluginDef, root: &Path) -> Vec<PathRule> {
    let mut out = Vec::new();
    for rule in &ext.manifest_entries {
        evaluate_rule(&ext.name, rule, root, &mut out);
    }
    out
}

fn evaluate_rule(
    plugin_name: &str,
    rule: &ManifestEntryRule,
    root: &Path,
    out: &mut Vec<PathRule>,
) {
    if rule.entries.is_empty() {
        tracing::warn!(
            "Plugin '{plugin_name}': manifestEntries rule for '{}' has an empty 'entries' list; \
             it seeds nothing.",
            rule.manifests
        );
        return;
    }

    let Ok(glob) = globset::Glob::new(&rule.manifests) else {
        // Glob validity is enforced at config load; a build failure here is defensive.
        tracing::warn!(
            "Plugin '{plugin_name}': manifestEntries 'manifests' glob '{}' failed to compile.",
            rule.manifests
        );
        return;
    };
    let matcher = glob.compile_matcher();

    let referenced = referenced_field_paths(rule);
    let mut resolved: BTreeMap<&str, bool> =
        referenced.iter().map(|p| (p.as_str(), false)).collect();

    let mut matched = 0usize;
    let mut passed = 0usize;
    let mut parse_failures = 0usize;

    for file in discover_manifest_paths(root, &matcher) {
        matched += 1;

        let Ok(source) = std::fs::read_to_string(&file) else {
            parse_failures += 1;
            continue;
        };
        let manifest: Value = match fallow_config::jsonc::parse_to_value(&source) {
            Ok(value) => value,
            Err(_) => {
                parse_failures += 1;
                continue;
            }
        };

        if !when_matches(&manifest, &rule.when) {
            continue;
        }
        passed += 1;

        for path in &referenced {
            if dotted_lookup(&manifest, path).is_some()
                && let Some(flag) = resolved.get_mut(path.as_str())
            {
                *flag = true;
            }
        }

        seed_entries(plugin_name, rule, &manifest, &file, root, out);
    }

    emit_diagnostics(
        plugin_name,
        rule,
        matched,
        passed,
        parse_failures,
        &resolved,
    );
}

/// Seed one manifest's entries into `out`.
fn seed_entries(
    plugin_name: &str,
    rule: &ManifestEntryRule,
    manifest: &Value,
    manifest_path: &Path,
    root: &Path,
    out: &mut Vec<PathRule>,
) {
    for seed in &rule.entries {
        if !when_matches(manifest, &seed.when) {
            continue;
        }
        for concrete in expand_interpolations(&seed.path, manifest) {
            match normalize_config_path(&concrete, manifest_path, root) {
                Some(rel) => out.push(PathRule::new(rel)),
                None => tracing::warn!(
                    "Plugin '{plugin_name}': manifestEntries entry '{concrete}' (from manifest \
                     '{}') resolved outside the project root and was skipped.",
                    manifest_path.display()
                ),
            }
        }
    }
}

fn emit_diagnostics(
    plugin_name: &str,
    rule: &ManifestEntryRule,
    matched: usize,
    passed: usize,
    parse_failures: usize,
    resolved: &BTreeMap<&str, bool>,
) {
    if matched == 0 {
        tracing::warn!(
            "Plugin '{plugin_name}': manifestEntries 'manifests' glob '{}' matched no files. \
             Check the glob and whether the manifests live under an ignored directory.",
            rule.manifests
        );
        return;
    }
    if parse_failures > 0 {
        tracing::warn!(
            "Plugin '{plugin_name}': manifestEntries skipped {parse_failures} manifest(s) that \
             could not be read or parsed (glob '{}').",
            rule.manifests
        );
    }
    if passed == 0 {
        // Nothing cleared the `when` gate; the per-field "resolved in none"
        // warning below would just be redundant noise, so stop here.
        tracing::warn!(
            "Plugin '{plugin_name}': manifestEntries 'when' gate excluded all {matched} manifest(s) \
             matched by '{}'. No entries were seeded.",
            rule.manifests
        );
        return;
    }
    for (path, was_resolved) in resolved {
        if !was_resolved {
            tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries field path '{path}' resolved in none of \
                 the {passed} gated manifest(s). Likely a typo in a 'when' key or a ${{...}} \
                 interpolation.",
            );
        }
    }
}

/// Collect every field path a rule references (rule-level `when` keys, per-seed
/// `when` keys, and `${...}` interpolations in seed paths) for typo diagnostics.
fn referenced_field_paths(rule: &ManifestEntryRule) -> Vec<String> {
    let mut paths: Vec<String> = rule.when.keys().cloned().collect();
    for seed in &rule.entries {
        paths.extend(seed.when.keys().cloned());
        paths.extend(interpolation_field_paths(&seed.path));
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Extract the dotted field paths named by `${...}` interpolations in a path.
fn interpolation_field_paths(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

/// Expand `${dotted.field}` interpolations in a path against a manifest, fanning
/// out over string / array field values. Returns an empty vec when any
/// interpolation resolves to nothing (a missing field seeds nothing).
fn expand_interpolations(path: &str, manifest: &Value) -> Vec<String> {
    let Some(start) = path.find("${") else {
        return vec![path.to_string()];
    };
    let prefix = &path[..start];
    let after = &path[start + 2..];
    let Some(end) = after.find('}') else {
        // Unterminated interpolation: not a valid template, seed nothing.
        return Vec::new();
    };
    let field = &after[..end];
    let suffix = &after[end + 1..];

    // Recurse on the SUFFIX only (strictly shorter, so termination is
    // guaranteed) and cartesian-combine with this field's values. A substituted
    // value is treated as a literal segment, never re-scanned for `${...}`, so a
    // manifest whose field value contains `${...}` cannot cause runaway recursion.
    let mut out = Vec::new();
    let tails = expand_interpolations(suffix, manifest);
    for value in field_segment_values(manifest, field) {
        for tail in &tails {
            out.push(format!("{prefix}{value}{tail}"));
        }
    }
    out
}

/// The path-segment string values a dotted field yields: a string or number
/// yields one; an array yields one per scalar element; anything else yields none.
fn field_segment_values(manifest: &Value, field: &str) -> Vec<String> {
    match dotted_lookup(manifest, field) {
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        Some(Value::Number(n)) => vec![n.to_string()],
        Some(Value::Array(items)) => items.iter().filter_map(scalar_segment).collect(),
        _ => Vec::new(),
    }
}

fn scalar_segment(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Whether every `(dotted-path, expected)` pair in `when` matches the manifest
/// by strict equality. An empty map always matches.
fn when_matches(manifest: &Value, when: &BTreeMap<String, Value>) -> bool {
    when.iter()
        .all(|(path, expected)| dotted_lookup(manifest, path) == Some(expected))
}

/// Look up a dotted field path (`plugin.browser`) in a JSON value.
fn dotted_lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

/// Walk `root` (respecting `.gitignore`, skipping `node_modules`) and return the
/// absolute paths of files whose root-relative path matches `matcher`. Bounded
/// to the manifest glob; runs only when an active plugin declares manifestEntries.
fn discover_manifest_paths(root: &Path, matcher: &globset::GlobMatcher) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| entry.file_name() != "node_modules")
        .build();
    for entry in walker.flatten() {
        // Skip directories; match everything else (regular files and any
        // symlinked manifest) against the glob, reads fail gracefully.
        if entry.file_type().is_none_or(|ft| ft.is_dir()) {
            continue;
        }
        let path = entry.path();
        if let Some(rel) = root_relative_forward_slash(path, root)
            && matcher.is_match(Path::new(&rel))
        {
            out.push(path.to_path_buf());
        }
    }
    out
}

/// Root-relative forward-slash string for a discovered (absolute) path, or
/// `None` if it is not under `root`.
fn root_relative_forward_slash(file: &Path, root: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{EntryPointRole, ManifestFormat, ManifestSeedRule};

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn seed(path: &str, when: &[(&str, Value)]) -> ManifestSeedRule {
        ManifestSeedRule {
            path: path.to_string(),
            when: when
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn dotted_lookup_traverses_nested_fields() {
        let m = json(r#"{"plugin": {"browser": true, "id": "actions"}}"#);
        assert_eq!(
            dotted_lookup(&m, "plugin.browser"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            dotted_lookup(&m, "plugin.id"),
            Some(&Value::String("actions".into()))
        );
        assert_eq!(dotted_lookup(&m, "plugin.missing"), None);
        assert_eq!(dotted_lookup(&m, "absent.field"), None);
    }

    #[test]
    fn when_matches_is_strict_equality_and_presence_is_not_matched() {
        let m = json(r#"{"type": "plugin", "plugin": {"browser": false}}"#);
        let mut when = BTreeMap::new();
        when.insert("type".to_string(), Value::String("plugin".into()));
        assert!(when_matches(&m, &when));

        // browser is present but false: matching against `true` must FAIL
        // (strict equality, no presence overload).
        let mut when_browser = BTreeMap::new();
        when_browser.insert("plugin.browser".to_string(), Value::Bool(true));
        assert!(!when_matches(&m, &when_browser));

        // empty when always matches
        assert!(when_matches(&m, &BTreeMap::new()));
    }

    #[test]
    fn expand_interpolations_string_array_and_missing() {
        let m = json(r#"{"plugin": {"extraPublicDirs": ["common", "types"], "id": "actions"}}"#);
        // string field -> one entry
        assert_eq!(
            expand_interpolations("${plugin.id}/index.ts", &m),
            vec!["actions/index.ts"]
        );
        // array field -> one entry per element
        assert_eq!(
            expand_interpolations("${plugin.extraPublicDirs}/index.{ts,tsx}", &m),
            vec!["common/index.{ts,tsx}", "types/index.{ts,tsx}"]
        );
        // missing field -> nothing seeded
        assert!(expand_interpolations("${plugin.absent}/index.ts", &m).is_empty());
        // no interpolation -> passthrough
        assert_eq!(
            expand_interpolations("public/index.{ts,tsx}", &m),
            vec!["public/index.{ts,tsx}"]
        );
    }

    #[test]
    fn evaluate_seeds_relative_to_manifest_dir_with_when_and_fanout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest_dir = root.join("x-pack/plugins/actions");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        let manifest_path = manifest_dir.join("kibana.jsonc");
        std::fs::write(
            &manifest_path,
            r#"{
                // a real Kibana-shaped manifest
                "type": "plugin",
                "plugin": { "browser": true, "server": false, "extraPublicDirs": ["common"] },
            }"#,
        )
        .unwrap();

        let ext = ExternalPluginDef {
            schema: None,
            name: "kibana".to_string(),
            detection: None,
            enablers: vec![],
            entry_points: vec![],
            entry_point_role: EntryPointRole::Runtime,
            manifest_entries: vec![ManifestEntryRule {
                manifests: "**/kibana.jsonc".to_string(),
                format: ManifestFormat::Jsonc,
                when: BTreeMap::from([("type".to_string(), Value::String("plugin".into()))]),
                entries: vec![
                    seed(
                        "public/index.{ts,tsx}",
                        &[("plugin.browser", Value::Bool(true))],
                    ),
                    seed(
                        "server/index.{ts,tsx}",
                        &[("plugin.server", Value::Bool(true))],
                    ),
                    seed("${plugin.extraPublicDirs}/index.{ts,tsx}", &[]),
                ],
            }],
            config_patterns: vec![],
            always_used: vec![],
            tooling_dependencies: vec![],
            used_exports: vec![],
            used_class_members: vec![],
        };

        let rules = evaluate_manifest_entries(&ext, root);
        let paths: Vec<&str> = rules.iter().map(|r| r.pattern.as_str()).collect();

        // browser:true seeds public; server:false does NOT seed server; extraPublicDirs fans out.
        assert!(paths.contains(&"x-pack/plugins/actions/public/index.{ts,tsx}"));
        assert!(paths.contains(&"x-pack/plugins/actions/common/index.{ts,tsx}"));
        assert!(
            !paths.iter().any(|p| p.contains("server/index")),
            "server:false must not seed the server entry, got {paths:?}"
        );
    }
}
