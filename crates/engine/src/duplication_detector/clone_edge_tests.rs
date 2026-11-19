//! Behavior tests for the first and last token of a reported clone.

use super::*;
use crate::discover::FileId;

/// Twelve CSS rules. Rule 5 ends with `last5` instead of `display: block;`.
fn css_rules(last5: &str) -> String {
    use std::fmt::Write as _;
    let mut css = String::new();
    for i in 0..12 {
        let last = if i == 5 { last5 } else { "display: block;" };
        let _ = write!(
            css,
            ".c{i} {{\n  margin: {i}px;\n  padding: 4px 8px;\n  color: #333;\n  {last}\n}}\n"
        );
    }
    css
}

fn line_ranges(dir: &std::path::Path, files: &[(&str, String)]) -> Vec<Vec<String>> {
    let discovered: Vec<DiscoveredFile> = files
        .iter()
        .enumerate()
        .map(|(index, (name, content))| {
            let path = dir.join(name);
            std::fs::write(&path, content).expect("write fixture file");
            DiscoveredFile {
                id: FileId(u32::try_from(index).expect("small index")),
                path,
                size_bytes: content.len() as u64,
            }
        })
        .collect();
    let report = find_duplicates(dir, &discovered, &DuplicatesConfig::default());
    let mut groups: Vec<Vec<String>> = report
        .clone_groups
        .iter()
        .map(|group| {
            let mut instances: Vec<String> = group
                .instances
                .iter()
                .map(|instance| {
                    let file = instance.file.strip_prefix(dir).unwrap_or(&instance.file);
                    format!(
                        "{}:{}-{}",
                        file.to_string_lossy(),
                        instance.start_line,
                        instance.end_line
                    )
                })
                .collect();
            instances.sort();
            instances
        })
        .collect();
    groups.sort();
    groups
}

#[test]
fn css_clone_does_not_start_on_the_semicolon_of_a_different_declaration() {
    let dir = tempfile::tempdir().expect("temp dir");
    let groups = line_ranges(
        dir.path(),
        &[
            ("a.css", css_rules("text-wrap: balance;")),
            ("b.css", css_rules("max-width: 30ch;")),
        ],
    );

    assert_eq!(
        groups,
        vec![
            vec!["a.css:1-34".to_string(), "b.css:1-34".to_string()],
            vec!["a.css:36-72".to_string(), "b.css:36-72".to_string()],
        ]
    );
}
