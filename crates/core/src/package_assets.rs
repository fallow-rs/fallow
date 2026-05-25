use std::path::{Component, Path};

use fallow_config::PackageJson;

pub const PACKAGE_FILES_SOURCE: &str = "package-files";

pub fn scaffold_template_asset_patterns(pkg: &PackageJson) -> Vec<String> {
    if pkg.files.iter().any(|entry| entry.trim().starts_with('!')) {
        return Vec::new();
    }

    pkg.files
        .iter()
        .filter_map(|entry| scaffold_template_asset_pattern(entry))
        .collect()
}

fn scaffold_template_asset_pattern(entry: &str) -> Option<String> {
    let normalized = normalize_publish_entry(entry)?;
    if !has_template_or_scaffold_segment(&normalized) || looks_like_file(&normalized) {
        return None;
    }

    if normalized.ends_with("/**") || normalized.ends_with("/**/*") {
        return Some(normalized);
    }

    Some(format!("{normalized}/**/*"))
}

fn normalize_publish_entry(entry: &str) -> Option<String> {
    let mut normalized = entry.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }

    if normalized.is_empty()
        || normalized.starts_with('!')
        || normalized.contains(':')
        || Path::new(&normalized).is_absolute()
        || has_parent_or_prefix_component(&normalized)
    {
        return None;
    }

    normalized = normalized.trim_matches('/').to_string();

    Some(normalized)
}

fn has_parent_or_prefix_component(pattern: &str) -> bool {
    Path::new(pattern)
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
}

fn has_template_or_scaffold_segment(pattern: &str) -> bool {
    pattern.split('/').any(|segment| {
        let segment = segment
            .trim_matches('*')
            .trim_end_matches('{')
            .trim_end_matches('}');
        segment.starts_with("template") || segment.starts_with("scaffold")
    })
}

fn looks_like_file(pattern: &str) -> bool {
    let trimmed = pattern.trim_end_matches("/**").trim_end_matches("/**/*");
    let Some(last_segment) = trimmed.rsplit('/').next() else {
        return false;
    };
    !last_segment.contains('*') && Path::new(last_segment).extension().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(files: &[&str]) -> PackageJson {
        PackageJson {
            files: files.iter().map(|value| (*value).to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn package_files_template_roots_become_recursive_patterns() {
        assert_eq!(
            scaffold_template_asset_patterns(&pkg(&[
                "template-*",
                "templates/**",
                "templates",
                "scaffold/**",
                "scaffolds/*",
            ])),
            vec![
                "template-*/**/*",
                "templates/**",
                "templates/**/*",
                "scaffold/**",
                "scaffolds/*/**/*",
            ]
        );
    }

    #[test]
    fn package_files_generic_publish_entries_are_ignored() {
        assert!(
            scaffold_template_asset_patterns(&pkg(&[
                "dist",
                "index.js",
                "README.md",
                "src/index.ts"
            ]))
            .is_empty()
        );
    }

    #[test]
    fn package_files_reject_absolute_traversal_and_file_entries() {
        assert!(
            scaffold_template_asset_patterns(&pkg(&[
                "/template-react",
                "C:/template-react",
                "../template-react",
                "templates/../template-react",
                "template.json",
            ]))
            .is_empty()
        );
    }

    #[test]
    fn package_files_with_negated_entries_skip_derivation() {
        assert!(
            scaffold_template_asset_patterns(&pkg(&["template-*", "!template-legacy"])).is_empty()
        );
    }

    #[test]
    fn package_files_normalize_relative_prefixes_and_separators() {
        assert_eq!(
            scaffold_template_asset_patterns(&pkg(&["./template-react", r".\scaffold"])),
            vec!["template-react/**/*", "scaffold/**/*"]
        );
    }
}
