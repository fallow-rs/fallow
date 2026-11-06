//! Select the targets of a package.json `exports` or `imports` map entry for
//! the active conditions.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PackageMapTarget {
    NoMatch,
    Blocked,
    Targets(Vec<String>),
}

fn package_map_match_value(
    value: &Value,
    condition_names: &[String],
    capture: Option<&str>,
) -> PackageMapTarget {
    resolve_package_map_value(value, condition_names, capture)
        .filter(|targets| !targets.is_empty())
        .map_or(PackageMapTarget::Blocked, PackageMapTarget::Targets)
}

pub(super) fn package_map_target(
    map: &Value,
    specifier_key: &str,
    condition_names: &[String],
) -> PackageMapTarget {
    let Some(obj) = map.as_object() else {
        if specifier_key == "." {
            return package_map_match_value(map, condition_names, None);
        }
        return PackageMapTarget::NoMatch;
    };

    let has_subpath_keys = obj
        .keys()
        .any(|key| key == "." || key.starts_with("./") || key.starts_with('#'));
    if !has_subpath_keys {
        if specifier_key == "." {
            return package_map_match_value(map, condition_names, None);
        }
        return PackageMapTarget::NoMatch;
    }

    if let Some(value) = obj.get(specifier_key) {
        return package_map_match_value(value, condition_names, None);
    }

    let mut patterns: Vec<(&str, &Value, String)> = obj
        .iter()
        .filter_map(|(pattern, value)| {
            package_map_pattern_capture(pattern, specifier_key)
                .map(|capture| (pattern.as_str(), value, capture))
        })
        .collect();
    patterns.sort_by(|(left, _, _), (right, _, _)| {
        package_map_pattern_specificity(right).cmp(&package_map_pattern_specificity(left))
    });

    patterns
        .first()
        .map_or(PackageMapTarget::NoMatch, |(_, value, capture)| {
            package_map_match_value(value, condition_names, Some(capture))
        })
}

pub(super) fn resolve_package_map_value(
    value: &Value,
    condition_names: &[String],
    capture: Option<&str>,
) -> Option<Vec<String>> {
    match value {
        Value::String(target) => Some(vec![match capture {
            Some(capture) => target.replace('*', capture),
            None => target.clone(),
        }]),
        // Every active condition contributes its targets in manifest order. A
        // caller tries them in turn, so a `types` branch with an unbuilt
        // declaration file falls through to a later `source` branch.
        Value::Object(map) => flatten_package_map_values(
            map.iter()
                .filter(|(condition, _)| {
                    *condition == "default" || condition_names.iter().any(|name| name == *condition)
                })
                .map(|(_, value)| value),
            condition_names,
            capture,
        ),
        Value::Array(values) => flatten_package_map_values(values.iter(), condition_names, capture),
        Value::Bool(_) | Value::Null | Value::Number(_) => None,
    }
}

fn flatten_package_map_values<'a>(
    values: impl Iterator<Item = &'a Value>,
    condition_names: &[String],
    capture: Option<&str>,
) -> Option<Vec<String>> {
    let targets: Vec<String> = values
        .filter_map(|value| resolve_package_map_value(value, condition_names, capture))
        .flatten()
        .collect();
    (!targets.is_empty()).then_some(targets)
}

pub(super) fn package_map_pattern_capture(pattern: &str, specifier: &str) -> Option<String> {
    let star = pattern.find('*')?;
    if pattern[star + 1..].contains('*') {
        return None;
    }
    let (prefix, suffix_with_star) = pattern.split_at(star);
    let suffix = &suffix_with_star[1..];
    let captured = specifier.strip_prefix(prefix)?.strip_suffix(suffix)?;
    Some(captured.to_string())
}

fn package_map_pattern_specificity(pattern: &str) -> (usize, usize) {
    let star = pattern.find('*').unwrap_or(pattern.len());
    (star, pattern.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditions() -> Vec<String> {
        [
            "development",
            "import",
            "require",
            "default",
            "types",
            "node",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    #[test]
    fn package_map_exact_entry_beats_pattern_entry() {
        let map = serde_json::json!({
            "#nitro/runtime/task": "./dist/special/task.mjs",
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Targets(vec!["./dist/special/task.mjs".to_string()])
        );
    }

    #[test]
    fn package_map_wildcard_substitutes_capture() {
        let map = serde_json::json!({
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Targets(vec!["./dist/runtime/internal/task.mjs".to_string()])
        );
    }

    #[test]
    fn package_map_exact_entry_with_no_target_blocks_pattern_entry() {
        let map = serde_json::json!({
            "#nitro/runtime/task": null,
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Blocked
        );
    }

    #[test]
    fn package_map_best_pattern_with_no_target_blocks_broader_pattern() {
        let map = serde_json::json!({
            "#nitro/runtime/internal/*": null,
            "#nitro/runtime/*": "./dist/runtime/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/internal/task", &conditions()),
            PackageMapTarget::Blocked
        );
    }

    #[test]
    fn package_map_unmatched_subpath_is_not_a_target() {
        let map = serde_json::json!({
            "./query": "./dist/query/index.js"
        });
        assert_eq!(
            package_map_target(&map, "./private", &conditions()),
            PackageMapTarget::NoMatch
        );
    }

    #[test]
    fn package_map_nested_conditions_list_targets_in_manifest_order() {
        let map = serde_json::json!({
            "./query/react": {
                "types": "./dist/query/react/index.d.ts",
                "import": {
                    "development": "./src/query/react/index.ts",
                    "default": "./dist/query/react/index.js"
                },
                "default": "./dist/query/react/index.cjs"
            }
        });
        assert_eq!(
            package_map_target(&map, "./query/react", &conditions()),
            PackageMapTarget::Targets(vec![
                "./dist/query/react/index.d.ts".to_string(),
                "./src/query/react/index.ts".to_string(),
                "./dist/query/react/index.js".to_string(),
                "./dist/query/react/index.cjs".to_string(),
            ])
        );
    }

    #[test]
    fn package_map_import_before_types_lists_runtime_branch_first() {
        let map = serde_json::json!({
            ".": {
                "import": "./dist/index.js",
                "types": "./dist/index.d.ts"
            }
        });
        assert_eq!(
            package_map_target(&map, ".", &conditions()),
            PackageMapTarget::Targets(vec![
                "./dist/index.js".to_string(),
                "./dist/index.d.ts".to_string()
            ])
        );
    }

    #[test]
    fn package_map_condition_order_follows_manifest_order() {
        let map = serde_json::json!({
            ".": {
                "node": "./dist/node.js",
                "import": "./dist/index.js"
            }
        });
        assert_eq!(
            package_map_target(&map, ".", &conditions()),
            PackageMapTarget::Targets(vec![
                "./dist/node.js".to_string(),
                "./dist/index.js".to_string()
            ])
        );
    }

    #[test]
    fn package_map_inactive_conditions_add_no_targets() {
        let map = serde_json::json!({
            "./admin": {
                "types": "./dist/admin/index.d.ts",
                "source": "./admin/index.ts",
                "default": "./dist/admin/index.js"
            }
        });
        assert_eq!(
            package_map_target(&map, "./admin", &conditions()),
            PackageMapTarget::Targets(vec![
                "./dist/admin/index.d.ts".to_string(),
                "./dist/admin/index.js".to_string()
            ])
        );
    }

    #[test]
    fn package_map_arrays_preserve_fallback_order() {
        let map = serde_json::json!({
            "#array": ["./dist/missing.js", "./src/array.ts"],
            "#null": null,
            "#false": false
        });
        assert_eq!(
            package_map_target(&map, "#array", &conditions()),
            PackageMapTarget::Targets(vec![
                "./dist/missing.js".to_string(),
                "./src/array.ts".to_string()
            ])
        );
        assert_eq!(
            package_map_target(&map, "#null", &conditions()),
            PackageMapTarget::Blocked
        );
        assert_eq!(
            package_map_target(&map, "#false", &conditions()),
            PackageMapTarget::Blocked
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn package_imports_fall_through_to_a_later_condition() {
        use std::path::PathBuf;

        use fallow_types::discover::FileId;

        let root = PathBuf::from("/project");
        let source = root.join("src/x.ts");
        let package_json = fallow_config::PackageJson {
            imports: Some(serde_json::json!({
                "#x": { "import": "./missing/x.js", "default": "./src/x.ts" }
            })),
            ..Default::default()
        };
        super::super::tests::with_package_map_ctx(
            root.clone(),
            None,
            package_json,
            &[(source, FileId(3))],
            |ctx, _, _| {
                let result =
                    super::super::try_package_imports_fallback(ctx, &root.join("src/a.ts"), "#x");
                assert!(
                    matches!(
                        result,
                        Some(crate::resolve::ResolveResult::InternalModule(FileId(3)))
                    ),
                    "{result:?}"
                );
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn missing_export_paths_skip_targets_that_exist() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("committed")).expect("create dir");
        std::fs::write(dir.path().join("committed/x.json"), "{}").expect("write file");
        let package_json = fallow_config::PackageJson {
            exports: Some(serde_json::json!({
                "./x": { "import": "./dist/x.mjs", "default": "./committed/x.json" }
            })),
            ..Default::default()
        };
        super::super::tests::with_package_map_ctx(
            dir.path().to_path_buf(),
            Some("pkg"),
            package_json,
            &[],
            |ctx, _, root| {
                assert_eq!(
                    super::super::missing_package_export_paths(ctx, "pkg/x"),
                    Some(vec![root.join("dist/x.mjs")])
                );
            },
        );
    }
}
