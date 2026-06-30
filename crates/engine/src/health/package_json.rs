//! Package.json probes used by health CSS analytics.

use std::path::Path;

use fallow_config::ResolvedConfig;
use rustc_hash::FxHashSet;

/// Returns `true` when the project's root `package.json` declares a Tailwind
/// dependency (`tailwindcss` or any `@tailwindcss/*`).
pub(super) fn project_uses_tailwind(root: &Path) -> bool {
    let Ok(json) = root_package_json(root) else {
        return false;
    };
    ["dependencies", "devDependencies", "peerDependencies"]
        .iter()
        .any(|key| {
            json.get(key)
                .and_then(serde_json::Value::as_object)
                .is_some_and(|deps| {
                    deps.keys()
                        .any(|k| k == "tailwindcss" || k.starts_with("@tailwindcss/"))
                })
        })
}

/// Normalized names of the project's declared dependencies (length-floored),
/// used to abstain on third-party CSS classes a library applies to its own DOM.
pub(super) fn dependency_class_prefixes(config: &ResolvedConfig) -> FxHashSet<String> {
    let mut prefixes = FxHashSet::default();
    let Ok(json) = root_package_json(&config.root) else {
        return prefixes;
    };
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(deps) = json.get(key).and_then(serde_json::Value::as_object) {
            for name in deps.keys() {
                let bare = name.rsplit('/').next().unwrap_or(name);
                let normalized = normalize_dep_token(bare);
                if normalized.len() >= MIN_DEP_PREFIX_LEN {
                    prefixes.insert(normalized);
                }
            }
        }
    }
    prefixes
}

/// True when a CSS class is likely a third-party library class.
pub(super) fn class_matches_dependency_prefix(
    class: &str,
    dependency_prefixes: &FxHashSet<String>,
) -> bool {
    if dependency_prefixes.is_empty() {
        return false;
    }
    let normalized = normalize_dep_token(class);
    dependency_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix.as_str()))
}

/// Project-root-relative CSS/SCSS paths published as package entry surfaces.
pub(super) fn published_css_paths(config: &ResolvedConfig) -> FxHashSet<String> {
    let mut published = FxHashSet::default();
    let Ok(json) = root_package_json(&config.root) else {
        return published;
    };
    let normalize = |s: &str| s.trim_start_matches("./").replace('\\', "/");
    let is_css = |s: &str| {
        matches!(
            Path::new(s)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("css" | "scss")
        )
    };
    for key in ["style", "main", "sass", "module"] {
        if let Some(s) = json.get(key).and_then(serde_json::Value::as_str)
            && is_css(s)
        {
            published.insert(normalize(s));
        }
    }
    let mut stack = vec![
        json.get("exports")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    ];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::String(s) if is_css(&s) => {
                published.insert(normalize(&s));
            }
            serde_json::Value::Array(items) => stack.extend(items),
            serde_json::Value::Object(map) => stack.extend(map.into_values()),
            _ => {}
        }
    }
    published
}

/// True when the project declares a Tailwind plugin through CSS or config.
pub(super) fn project_uses_tailwind_plugin(any_plugin_directive: bool, root: &Path) -> bool {
    if any_plugin_directive {
        return true;
    }
    for name in [
        "tailwind.config.js",
        "tailwind.config.ts",
        "tailwind.config.mjs",
        "tailwind.config.cjs",
        "tailwind.config.mts",
        "tailwind.config.cts",
    ] {
        if let Ok(text) = std::fs::read_to_string(root.join(name))
            && text_has_nonempty_plugins_array(&text)
        {
            return true;
        }
    }
    false
}

const MIN_DEP_PREFIX_LEN: usize = 6;

fn root_package_json(root: &Path) -> Result<serde_json::Value, std::io::Error> {
    let text = std::fs::read_to_string(root.join("package.json"))?;
    serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

fn normalize_dep_token(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// True when a `tailwind.config.*` text declares a non-empty `plugins` array.
fn text_has_nonempty_plugins_array(text: &str) -> bool {
    let bytes = text.as_bytes();
    let skip_ws = |mut k: usize| {
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        k
    };
    let mut from = 0;
    while let Some(rel) = text[from..].find("plugins") {
        let mut k = skip_ws(from + rel + "plugins".len());
        if k < bytes.len() && bytes[k] == b':' {
            k = skip_ws(k + 1);
            if k < bytes.len() && bytes[k] == b'[' {
                k = skip_ws(k + 1);
                if k < bytes.len() && bytes[k] != b']' {
                    return true;
                }
            }
        }
        from += rel + "plugins".len();
    }
    false
}
