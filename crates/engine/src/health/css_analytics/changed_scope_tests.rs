//! A deep CSS run scans the whole project but reports only the changed
//! files. These tests check that an unchanged file cannot hide a finding of a
//! changed file.

use super::*;
use fallow_config::{FallowConfig, OutputFormat};
use fallow_types::discover::{DiscoveredFile, FileId};
use std::fmt::Write as _;
use std::path::Path;

fn config_at(root: &Path) -> ResolvedConfig {
    FallowConfig::default().resolve(root.to_path_buf(), OutputFormat::Human, 1, true, true, None)
}

fn write_file(root: &Path, id: u32, relative: &str, body: &str) -> DiscoveredFile {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    DiscoveredFile {
        id: FileId(id),
        size_bytes: u64::try_from(body.len()).unwrap(),
        path,
    }
}

fn tailwind_project(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
    )
    .unwrap();
}

/// The CSS report of a deep run with `changed` as the only reported file.
fn deep_report(
    root: &Path,
    files: &[DiscoveredFile],
    changed: &DiscoveredFile,
) -> fallow_output::CssAnalyticsReport {
    let config = config_at(root);
    let modules: Vec<fallow_types::extract::ModuleInfo> = files
        .iter()
        .map(|file| {
            let source = std::fs::read_to_string(&file.path).unwrap();
            fallow_extract::parse_source_to_module(file.id, &file.path, &source, 0, false)
        })
        .collect();
    let mut output_changed = rustc_hash::FxHashSet::default();
    output_changed.insert(changed.path.clone());
    compute_css_analytics_report(
        files,
        &modules,
        HealthScanCtx {
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            output_changed_files: Some(&output_changed),
            ws_roots: None,
        },
    )
    .unwrap()
    .report
}

#[test]
fn an_arbitrary_value_in_a_changed_file_is_reported_when_an_unchanged_file_has_it_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    tailwind_project(root);
    let markup = "export const A = () => <div className=\"p-[13px]\" />;\n";
    let first = write_file(root, 0, "src/a.tsx", markup);
    let changed = write_file(root, 1, "src/b.tsx", markup);

    let report = deep_report(root, &[first, changed.clone()], &changed);
    let values: Vec<(&str, &str, u32)> = report
        .tailwind_arbitrary_values
        .iter()
        .map(|value| (value.value.as_str(), value.path.as_str(), value.count))
        .collect();
    assert_eq!(values, vec![("p-[13px]", "src/b.tsx", 2)]);
}

#[test]
fn raw_style_values_of_unchanged_files_do_not_use_up_the_report_limit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut unchanged_css = String::new();
    for index in 0..=MAX_REPORTED_RAW_STYLE_VALUES {
        writeln!(unchanged_css, ".a{index} {{ color: #10{index:04x}; }}").unwrap();
    }
    let unchanged = write_file(root, 0, "src/a.css", &unchanged_css);
    let changed = write_file(root, 1, "src/b.css", ".b { color: #abcdef; }\n");

    let report = deep_report(root, &[unchanged, changed.clone()], &changed);
    let values: Vec<(&str, &str)> = report
        .raw_style_values
        .iter()
        .map(|raw| (raw.value.as_str(), raw.path.as_str()))
        .collect();
    assert_eq!(values, vec![("#abcdef", "src/b.css")]);
    assert_eq!(report.summary.raw_style_values, 1);
}

#[test]
fn a_plugin_directive_in_a_comment_or_string_is_not_a_plugin() {
    for source in [
        "/* @plugin \"@tailwindcss/typography\"; */\n@theme {\n  --color-brand: #f00;\n}",
        ".a { content: \"@plugin\"; }\n@theme {\n  --color-brand: #f00;\n}",
        ".a { color: red; } /* @plugin \"x\"; */",
        "@plugins \"x\";",
    ] {
        let mut tokens = CssTokenSets::default();
        tokens.record_theme(source, "src/theme.css");
        assert!(!tokens.any_plugin_directive, "{source}");
    }
    for source in [
        "@plugin \"@tailwindcss/typography\";\n@theme {\n  --color-brand: #f00;\n}",
        "@plugin './local.js';",
    ] {
        let mut tokens = CssTokenSets::default();
        tokens.record_theme(source, "src/theme.css");
        assert!(tokens.any_plugin_directive, "{source}");
    }
}

#[test]
fn raw_values_of_unchanged_files_still_name_the_nearest_token() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let unchanged = write_file(
        root,
        0,
        "src/a.css",
        ".a { color: #ff0000; }\n.b { color: #ff0000; }\n",
    );
    let changed = write_file(root, 1, "src/c.css", ".c { color: #ff0001; }\n");

    let report = deep_report(root, &[unchanged, changed.clone()], &changed);
    let nearest: Vec<(&str, Option<&str>)> = report
        .raw_style_values
        .iter()
        .map(|raw| {
            (
                raw.value.as_str(),
                raw.nearest_token.as_ref().map(|token| token.name.as_str()),
            )
        })
        .collect();
    let whole = {
        let config = config_at(root);
        let files = [
            write_file(
                root,
                0,
                "src/a.css",
                ".a { color: #ff0000; }\n.b { color: #ff0000; }\n",
            ),
            changed,
        ];
        compute_css_analytics_report(
            &files,
            &[],
            HealthScanCtx {
                config: &config,
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                output_changed_files: None,
                ws_roots: None,
            },
        )
        .unwrap()
        .report
    };
    let whole_nearest = whole
        .raw_style_values
        .iter()
        .find(|raw| raw.value == "#ff0001")
        .and_then(|raw| raw.nearest_token.as_ref())
        .map(|token| token.name.clone());
    assert!(
        whole_nearest.is_some(),
        "the whole-project run names a token"
    );
    assert_eq!(nearest, vec![("#ff0001", whole_nearest.as_deref())]);
}

#[test]
fn the_first_use_in_a_changed_file_stays_the_location() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    tailwind_project(root);
    let markup = "export const A = () => <div className=\"p-[13px]\" />;\n";
    let unchanged = write_file(root, 0, "src/a.tsx", markup);
    let first = write_file(root, 1, "src/b.tsx", markup);
    let second = write_file(root, 2, "src/c.tsx", markup);
    let config = config_at(root);
    let mut output_changed = rustc_hash::FxHashSet::default();
    output_changed.insert(first.path.clone());
    output_changed.insert(second.path.clone());
    let report = compute_css_analytics_report(
        &[unchanged, first, second],
        &[],
        HealthScanCtx {
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            output_changed_files: Some(&output_changed),
            ws_roots: None,
        },
    )
    .unwrap()
    .report;
    let paths: Vec<&str> = report
        .tailwind_arbitrary_values
        .iter()
        .map(|value| value.path.as_str())
        .collect();
    assert_eq!(paths, vec!["src/b.tsx"]);
}
