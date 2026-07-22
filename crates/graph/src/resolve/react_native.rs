//! React Native and Expo platform extension support.

use super::types::RN_PLATFORM_PREFIXES;

/// Check if React Native or Expo plugins are active.
fn has_react_native_plugin(active_plugins: &[String]) -> bool {
    active_plugins
        .iter()
        .any(|p| p == "react-native" || p == "expo")
}

/// Build the resolver extension list, optionally prepending React Native platform
/// extensions when the RN/Expo plugin is active.
pub(super) fn build_extensions(active_plugins: &[String]) -> Vec<String> {
    let base: Vec<String> = vec![
        ".ts".into(),
        ".tsx".into(),
        ".mts".into(),
        ".cts".into(),
        ".gts".into(),
        ".js".into(),
        ".jsx".into(),
        ".mjs".into(),
        ".cjs".into(),
        ".gjs".into(),
        ".d.ts".into(),
        ".d.mts".into(),
        ".d.cts".into(),
        ".json".into(),
        ".vue".into(),
        ".svelte".into(),
        ".astro".into(),
        ".mdx".into(),
        ".css".into(),
        ".scss".into(),
        ".graphql".into(),
        ".gql".into(),
    ];

    if has_react_native_plugin(active_plugins) {
        let source_exts = [".ts", ".tsx", ".js", ".jsx"];
        let mut rn_extensions: Vec<String> = Vec::new();
        for platform in RN_PLATFORM_PREFIXES {
            for ext in &source_exts {
                rn_extensions.push(format!("{platform}{ext}"));
            }
        }
        rn_extensions.extend(base);
        rn_extensions
    } else {
        base
    }
}

/// Build the resolver `condition_names` list.
///
/// Baseline conditions (in priority order): `development`, `import`, `require`,
/// `default`, `types`, `node`. `development` is included so that package.json
/// `exports` / `imports` entries declaring a `development` branch (a widely
/// used community condition, supported by Vite, Vitest, esbuild, and Rollup)
/// resolve to their source files instead of compiled `dist/` output. See
/// <https://nodejs.org/api/packages.html#community-conditions-definitions>.
///
/// When the React Native or Expo plugin is active, `react-native` and
/// `browser` are prepended ahead of the baseline for Metro-style resolution.
/// User-supplied `extra_conditions` are prepended ahead of everything else
/// so they take highest priority.
pub(super) fn build_condition_names(
    active_plugins: &[String],
    extra_conditions: &[String],
) -> Vec<String> {
    let mut names = vec![
        "development".into(),
        "import".into(),
        "require".into(),
        "default".into(),
        "types".into(),
        "node".into(),
    ];
    if has_react_native_plugin(active_plugins) {
        names.insert(0, "react-native".into());
        names.insert(1, "browser".into());
    }
    for extra in extra_conditions.iter().rev() {
        names.insert(0, extra.clone());
    }
    let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    names.retain(|name| seen.insert(name.clone()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_react_native_plugin_active() {
        let plugins = vec!["react-native".to_string(), "typescript".to_string()];
        assert!(has_react_native_plugin(&plugins));
    }

    #[test]
    fn test_has_expo_plugin_active() {
        let plugins = vec!["expo".to_string(), "typescript".to_string()];
        assert!(has_react_native_plugin(&plugins));
    }

    #[test]
    fn test_has_react_native_plugin_inactive() {
        let plugins = vec!["nextjs".to_string(), "typescript".to_string()];
        assert!(!has_react_native_plugin(&plugins));
    }

    #[test]
    fn test_rn_platform_extensions_prepended() {
        let no_rn = build_extensions(&[]);
        let rn_plugins = vec!["react-native".to_string()];
        let with_rn = build_extensions(&rn_plugins);

        assert_eq!(no_rn[0], ".ts");

        assert_eq!(with_rn[0], ".web.ts");
        assert_eq!(with_rn[1], ".web.tsx");
        assert_eq!(with_rn[2], ".web.js");
        assert_eq!(with_rn[3], ".web.jsx");

        assert!(with_rn.len() > no_rn.len());
        assert_eq!(
            with_rn.len(),
            no_rn.len() + 16,
            "should add 16 platform extensions (4 platforms x 4 exts)"
        );
    }

    #[test]
    fn test_rn_condition_names_prepended() {
        let no_rn = build_condition_names(&[], &[]);
        let rn_plugins = vec!["react-native".to_string()];
        let with_rn = build_condition_names(&rn_plugins, &[]);

        assert_eq!(no_rn[0], "development");

        assert_eq!(with_rn[0], "react-native");
        assert_eq!(with_rn[1], "browser");
        assert_eq!(with_rn[2], "development");
    }

    #[test]
    fn test_development_condition_in_baseline() {
        let names = build_condition_names(&[], &[]);
        assert!(
            names.contains(&"development".to_string()),
            "`development` must be part of the default condition set"
        );
    }

    #[test]
    fn test_extra_conditions_prepended_before_baseline() {
        let names = build_condition_names(&[], &["worker".to_string(), "edge-light".to_string()]);
        assert_eq!(names[0], "worker");
        assert_eq!(names[1], "edge-light");
        assert_eq!(names[2], "development");
    }

    #[test]
    fn test_extra_conditions_prepended_before_rn() {
        let rn_plugins = vec!["react-native".to_string()];
        let names = build_condition_names(&rn_plugins, &["worker".to_string()]);
        assert_eq!(names[0], "worker");
        assert_eq!(names[1], "react-native");
        assert_eq!(names[2], "browser");
        assert_eq!(names[3], "development");
    }

    #[test]
    fn test_duplicate_baseline_condition_from_user_is_deduped() {
        let names = build_condition_names(&[], &["development".to_string()]);
        let dev_count = names.iter().filter(|n| *n == "development").count();
        assert_eq!(dev_count, 1, "`development` should appear exactly once");
        assert_eq!(
            names[0], "development",
            "user-supplied entry keeps its position"
        );
    }

    #[test]
    fn test_duplicate_user_conditions_are_deduped_preserving_first() {
        let names = build_condition_names(
            &[],
            &[
                "worker".to_string(),
                "edge-light".to_string(),
                "worker".to_string(),
            ],
        );
        let worker_count = names.iter().filter(|n| *n == "worker").count();
        assert_eq!(worker_count, 1);
        assert_eq!(names[0], "worker");
        assert_eq!(names[1], "edge-light");
    }
}
