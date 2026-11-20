//! Packages that an Expo app uses with no import.
//!
//! - A font file path in the app config names the package that holds the
//!   file: `"@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf"`.
//! - Expo autolinking adds each installed package with an
//!   `expo-module.config.json` file to the native build. Such a package with
//!   no JavaScript entry is native only, so no module can import it.
//!
//! Both credits apply only to a package that the `package.json` beside the
//! app config declares and that is installed, so neither can name a package
//! that the project does not have.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::PluginResult;

const AUTOLINKING_MANIFEST: &str = "expo-module.config.json";

/// The `package.json` keys that give a package a JavaScript or type entry.
const ENTRY_KEYS: &[&str] = &[
    "main",
    "module",
    "exports",
    "types",
    "react-native",
    "source",
];

const INDEX_FILES: &[&str] = &[
    "index.js",
    "index.mjs",
    "index.cjs",
    "index.jsx",
    "index.ts",
    "index.tsx",
    "index.native.js",
    "index.ios.js",
    "index.android.js",
];

/// A quoted path to a font file whose first segments can name a package,
/// with an optional `node_modules/` or `./node_modules/` prefix.
#[expect(
    clippy::expect_used,
    reason = "the static font path pattern is hard-coded and covered by plugin tests"
)]
static FONT_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"["'`](?:(?:\./)?node_modules/)?((?:@[A-Za-z0-9._-]+/)?[A-Za-z0-9._-]+)/[^"'`\s]+\.(?:ttf|otf|woff2?)["'`]"#,
    )
    .expect("the font path pattern is valid")
});

/// Credit the declared packages that the app config or autolinking uses.
/// The search for an installed package stops at `project_root`.
pub(super) fn add_installed_package_credits(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    project_root: &Path,
) {
    let Some(app_dir) = config_path.parent() else {
        return;
    };
    let declared = declared_dependencies(app_dir);
    if declared.is_empty() {
        return;
    }
    for capture in FONT_PATH.captures_iter(source) {
        let package = &capture[1];
        if declared.iter().any(|name| name == package)
            && installed_package_dir(app_dir, project_root, package).is_some()
        {
            result.referenced_dependencies.push(package.to_string());
        }
    }
    for package in &declared {
        if installed_package_dir(app_dir, project_root, package)
            .is_some_and(|dir| is_native_only(&dir))
        {
            result.referenced_dependencies.push(package.clone());
        }
    }
}

/// The dependency names of the `package.json` in `app_dir`.
fn declared_dependencies(app_dir: &Path) -> Vec<String> {
    let Some(manifest) = read_json(&app_dir.join("package.json")) else {
        return Vec::new();
    };
    ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .filter_map(|section| manifest.get(section).and_then(Value::as_object))
        .flat_map(|section| section.keys().cloned())
        .collect()
}

/// The folder of `package` in the nearest `node_modules` at or above
/// `app_dir`, up to the project root.
fn installed_package_dir(app_dir: &Path, root: &Path, package: &str) -> Option<PathBuf> {
    let mut current = Some(app_dir);
    while let Some(dir) = current {
        let candidate = dir.join("node_modules").join(package);
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
        if dir == root || !dir.starts_with(root) {
            return None;
        }
        current = dir.parent();
    }
    None
}

/// Whether the package in `dir` is an Expo native module with no JavaScript
/// entry: it has an autolinking manifest, no entry key and no index file.
fn is_native_only(dir: &Path) -> bool {
    if !dir.join(AUTOLINKING_MANIFEST).is_file() {
        return false;
    }
    let Some(manifest) = read_json(&dir.join("package.json")) else {
        return false;
    };
    !ENTRY_KEYS.iter().any(|key| manifest.get(key).is_some())
        && !INDEX_FILES.iter().any(|file| dir.join(file).is_file())
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font_packages(source: &str) -> Vec<&str> {
        FONT_PATH
            .captures_iter(source)
            .map(|capture| capture.get(1).map_or("", |package| package.as_str()))
            .collect()
    }

    #[test]
    fn a_font_path_names_its_first_package_segments() {
        assert_eq!(
            font_packages(
                r#"const a = "@expo-google-fonts/dm-sans/400Regular/DMSans_400Regular.ttf";
                   const b = './node_modules/font-pkg/fonts/Body.otf';
                   const c = "assets/fonts/Local.woff2";"#
            ),
            ["@expo-google-fonts/dm-sans", "font-pkg", "assets"],
            "the caller keeps only the names that the manifest declares"
        );
    }

    #[test]
    fn a_hoisted_package_is_found_up_to_the_project_root() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        let app_dir = root.join("apps/mobile");
        let hoisted = root.join("node_modules/native-only");
        std::fs::create_dir_all(&app_dir).expect("app dir");
        std::fs::create_dir_all(&hoisted).expect("package dir");
        std::fs::write(hoisted.join("package.json"), r#"{ "name": "native-only" }"#)
            .expect("package manifest");
        std::fs::write(hoisted.join(AUTOLINKING_MANIFEST), "{}").expect("autolinking manifest");

        let found = installed_package_dir(&app_dir, root, "native-only");
        assert_eq!(found.as_deref(), Some(hoisted.as_path()));
        assert!(is_native_only(&hoisted));
        assert_eq!(
            installed_package_dir(&app_dir, &app_dir, "native-only"),
            None
        );

        std::fs::write(hoisted.join("index.ios.js"), "").expect("platform entry");
        assert!(
            !is_native_only(&hoisted),
            "a platform index file is a JavaScript entry"
        );
    }

    #[test]
    fn a_path_without_a_font_extension_names_no_package() {
        assert!(font_packages(r#"const a = "some-pkg/dist/index.js";"#).is_empty());
    }
}
