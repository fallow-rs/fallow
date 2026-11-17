//! An `import.meta.glob` with a raw or url query loads its matches as strings
//! or URLs, so it keeps the matches in use but credits none of their exports.
//! The `import` option credits only the named export (issue #4532).

use super::common::{create_config, fixture_path};

struct Findings {
    unused_files: Vec<String>,
    unused_exports: Vec<String>,
}

fn rel(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn findings() -> Findings {
    let config = create_config(fixture_path("issue-4532-import-meta-glob-query"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| rel(&finding.file.path, &config.root))
        .collect();
    unused_files.sort();
    let mut unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                rel(&finding.export.path, &config.root),
                finding.export.export_name
            )
        })
        .collect();
    unused_exports.sort();
    Findings {
        unused_files,
        unused_exports,
    }
}

fn unused_exports_in<'a>(findings: &'a Findings, dir: &str) -> Vec<&'a str> {
    let prefix = format!("src/{dir}/a.ts:");
    findings
        .unused_exports
        .iter()
        .filter_map(|entry| entry.strip_prefix(prefix.as_str()))
        .collect()
}

#[test]
fn raw_and_url_globs_keep_matches_in_use() {
    let findings = findings();
    for dir in ["raw", "url", "legacy-raw", "legacy-url", "raw-named"] {
        let file = format!("src/{dir}/a.ts");
        assert!(
            !findings.unused_files.contains(&file),
            "the `{dir}` glob loads {file}, found {:?}",
            findings.unused_files
        );
    }
}

#[test]
fn raw_and_url_globs_credit_no_export() {
    let findings = findings();
    for dir in ["url", "legacy-raw", "legacy-url", "raw-named"] {
        assert_eq!(
            unused_exports_in(&findings, dir),
            vec!["default", "setup", "unused"],
            "the `{dir}` glob reads its matches as text or a URL"
        );
    }
    assert_eq!(
        unused_exports_in(&findings, "raw"),
        vec!["default", "unused"],
        "only the static import credits `setup` of raw/a.ts"
    );
}

#[test]
fn negated_entry_of_a_raw_glob_stays_unused() {
    let findings = findings();
    assert!(
        findings
            .unused_files
            .contains(&"src/raw/skip.ts".to_string()),
        "`!./raw/skip.ts` excludes the file, found {:?}",
        findings.unused_files
    );
}

#[test]
fn import_option_credits_only_the_named_export() {
    let findings = findings();
    assert_eq!(
        unused_exports_in(&findings, "defaults"),
        vec!["setup", "unused"]
    );
    assert_eq!(
        unused_exports_in(&findings, "named"),
        vec!["default", "unused"]
    );
}

/// Vite applies `?inline` only to CSS, asset files and workers. A script
/// module with `?inline` still loads as a module.
#[test]
fn inline_query_on_script_modules_credits_every_export() {
    let findings = findings();
    assert!(
        !findings
            .unused_files
            .contains(&"src/inline/a.ts".to_string()),
        "the `inline` glob loads src/inline/a.ts, found {:?}",
        findings.unused_files
    );
    assert!(
        unused_exports_in(&findings, "inline").is_empty(),
        "`?inline` loads a `.ts` match as a module, found {:?}",
        findings.unused_exports
    );
}

#[test]
fn module_globs_keep_crediting_every_export() {
    let findings = findings();
    for dir in ["plain", "star", "inline", "custom", "dynamic-options"] {
        assert!(
            unused_exports_in(&findings, dir).is_empty(),
            "the `{dir}` glob loads its matches as modules, found {:?}",
            findings.unused_exports
        );
    }
}
