//! Files and packages that a tool uses by convention, with no import:
//! the CodeRabbit config file, the files of a GNOME Shell extension folder,
//! the Expo fingerprint config, a font package that the Expo app config names
//! by file path, and a native-only Expo module that autolinking adds.

use super::common::{create_config, fixture_path};

fn unused_files(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<String> {
    let mut files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

#[cfg_attr(miri, ignore)]
#[test]
fn coderabbit_config_and_gnome_extension_files_are_entry_points() {
    let root = fixture_path("tool-convention-entries");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_files(&results, &root),
        ["data/extension.js", "src/orphan.js"],
        "an extension.js beside a metadata.json of another tool stays unused"
    );
    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        unused_dev.is_empty(),
        "the CodeRabbit config imports its package, got {unused_dev:?}"
    );
}

#[cfg_attr(miri, ignore)]
#[test]
fn expo_credits_font_packages_and_native_only_modules() {
    let root = fixture_path("expo-installed-package-credits");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unused_files(&results, &root).is_empty(),
        "the Expo CLI loads fingerprint.config.js by name"
    );
    let mut unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    unused.sort_unstable();
    assert_eq!(
        unused,
        ["font-without-reference", "native-with-js"],
        "a font path credits its package, and only a native module with no JavaScript entry is credited without an import"
    );
}
