//! Apply config-level fixes for `duplicate_exports`.
//!
//! Two paths:
//!
//! - **Edit**: a fallow config file already exists at or above `root`.
//!   Append `ignoreExports` entries to it via `add_ignore_exports_rule`.
//! - **Create-fallback**: no fallow config file exists. Generate a fresh
//!   `.fallowrc.json` seed via the same scaffolding `fallow init` uses
//!   (framework detection, `$schema`, `entry`, `ignorePatterns`, etc.) and
//!   then layer the new `ignoreExports` entries on top so the user gets one
//!   coherent config instead of a thin `{ "ignoreExports": [...] }` shell.
//!
//! Either path refuses to act when the resolution lands inside a monorepo
//! subpackage with a workspace root somewhere above (`pnpm-workspace.yaml`,
//! `package.json#workspaces`, `turbo.json`, `lerna.json`); fragmenting
//! per-package configs across 8 sub-packages is a worse default than the
//! existing "skip and warn" behavior. The user must either run `fallow init`
//! at the workspace root or invoke `fallow fix` from there.
//!
//! `--no-create-config` (FixOptions::no_create_config) is the escape hatch
//! for pre-commit hooks, `fallow watch`, and CI bots that must NOT
//! materialize new top-level files.
//!
//! Dry-run output:
//!
//! - **Human mode**: prints a unified diff to stderr (hand-rolled
//!   `+`-prefix renderer for the create case; `similar::TextDiff::from_lines`
//!   for the edit case).
//! - **JSON mode**: the entry carries a `proposed_diff` field so agents
//!   piping `--format json` can validate the proposed write before passing
//!   `--yes`.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use fallow_config::{
    ConfigFixPlan as ResolvedConfigPlan, IgnoreExportRule, OutputFormat,
    add_ignore_exports_rule_to_string, classify_config_fix_plan as classify_plan,
};
use fallow_types::output_dead_code::DuplicateExportFinding;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use super::io::atomic_write;
use crate::init;

/// Inputs for [`apply_config_fixes`], bundled so the entry point takes one
/// parameter struct instead of seven (mirrors the `*FixInput` convention used
/// by the dependency and export fixers in this module).
pub(super) struct ConfigFixInput<'a> {
    pub(super) root: &'a Path,
    pub(super) config_path: Option<&'a PathBuf>,
    pub(super) results: &'a AnalysisResults,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) no_create_config: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

pub(super) fn apply_config_fixes(input: ConfigFixInput<'_>) -> bool {
    let ConfigFixInput {
        root,
        config_path,
        results,
        output,
        dry_run,
        no_create_config,
        fixes,
    } = input;
    if results.duplicate_exports.is_empty() {
        return false;
    }

    let plan = classify_plan(root, config_path, no_create_config);
    match plan {
        ResolvedConfigPlan::Edit { config_path } => apply_edit(
            root,
            &config_path,
            &results.duplicate_exports,
            output,
            dry_run,
            fixes,
        ),
        ResolvedConfigPlan::Create { target } => apply_create(
            root,
            &target,
            &results.duplicate_exports,
            output,
            dry_run,
            fixes,
        ),
        ResolvedConfigPlan::BlockedMonorepo { workspace_root } => {
            emit_blocked_monorepo(root, &workspace_root, output, fixes);
            false
        }
        ResolvedConfigPlan::BlockedNoCreate { target } => {
            emit_blocked_no_create(root, &target, output, fixes);
            false
        }
    }
}

fn apply_edit(
    root: &Path,
    config_path: &Path,
    duplicate_exports: &[DuplicateExportFinding],
    output: OutputFormat,
    dry_run: bool,
    fixes: &mut Vec<serde_json::Value>,
) -> bool {
    let entries = ignore_export_entries(root, config_path, duplicate_exports);
    if entries.is_empty() {
        return false;
    }
    let config_file = display_path(root, config_path);

    if dry_run {
        return apply_edit_dry_run(config_path, &config_file, &entries, output, fixes);
    }

    match fallow_config::add_ignore_exports_rule(config_path, &entries) {
        Ok(()) => {
            fixes.push(serde_json::json!({
                "type": "add_ignore_exports",
                "config_key": "ignoreExports",
                "file": config_file,
                "entries": entries,
                "applied": true,
            }));
            false
        }
        Err(e) => {
            eprintln!(
                "Error: failed to write ignoreExports rules to {}: {e}",
                config_path.display()
            );
            true
        }
    }
}

/// Render the dry-run preview (diff + JSON entry) for the config-edit path.
/// Returns the orchestrator's `had_write_error` bool.
fn apply_edit_dry_run(
    config_path: &Path,
    config_file: &str,
    entries: &[IgnoreExportRule],
    output: OutputFormat,
    fixes: &mut Vec<serde_json::Value>,
) -> bool {
    let current = match std::fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error: failed to read {config_file} for dry-run preview: {e}");
            return true;
        }
    };
    let proposed = match add_ignore_exports_rule_to_string(config_path, &current, entries) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error: failed to compute proposed config edit for {config_file}: {e}");
            return true;
        }
    };
    if current == proposed {
        return false;
    }
    let diff = render_unified_diff(config_file, &current, &proposed);
    let mut entry = serde_json::json!({
        "type": "add_ignore_exports",
        "config_key": "ignoreExports",
        "file": config_file,
        "entries": entries,
        "proposed_diff": diff,
    });
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Would append {} ignoreExports rule(s) to {config_file}:",
            entries.len()
        );
        eprintln!("{diff}");
    }
    if let Some(obj) = entry.as_object_mut() {
        obj.insert("dry_run".to_owned(), serde_json::Value::Bool(true));
    }
    fixes.push(entry);
    false
}

fn apply_create(
    root: &Path,
    target: &Path,
    duplicate_exports: &[DuplicateExportFinding],
    output: OutputFormat,
    dry_run: bool,
    fixes: &mut Vec<serde_json::Value>,
) -> bool {
    let entries = ignore_export_entries(root, target, duplicate_exports);
    if entries.is_empty() {
        return false;
    }
    let target_display = display_path(root, target);

    let info = init::detect_project(root);
    let seed = init::build_json_config(&info);
    let proposed = match add_ignore_exports_rule_to_string(target, &seed, &entries) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error: failed to render proposed {target_display} content: {e}");
            return true;
        }
    };

    if dry_run {
        apply_create_dry_run(&target_display, &entries, &proposed, output, fixes);
        return false;
    }

    if let Err(e) = atomic_write(target, proposed.as_bytes()) {
        eprintln!("Error: failed to create {target_display}: {e}");
        return true;
    }
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Created {target_display} with {} ignoreExports rule(s). Check it in alongside the source edits.",
            entries.len()
        );
    }
    fixes.push(serde_json::json!({
        "type": "add_ignore_exports",
        "config_key": "ignoreExports",
        "file": target_display,
        "entries": entries,
        "created_files": [target_display],
        "applied": true,
    }));
    false
}

/// Render the dry-run preview (diff + JSON entry) for the config-create path.
fn apply_create_dry_run(
    target_display: &str,
    entries: &[IgnoreExportRule],
    proposed: &str,
    output: OutputFormat,
    fixes: &mut Vec<serde_json::Value>,
) {
    let diff = render_create_diff(target_display, proposed);
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Would create {target_display} with {} ignoreExports rule(s):",
            entries.len()
        );
        eprintln!("{diff}");
    }
    fixes.push(serde_json::json!({
        "type": "add_ignore_exports",
        "config_key": "ignoreExports",
        "file": target_display,
        "entries": entries,
        "proposed_diff": diff,
        "created_files": [target_display],
        "dry_run": true,
    }));
}

fn emit_blocked_monorepo(
    root: &Path,
    workspace_root: &Path,
    output: OutputFormat,
    fixes: &mut Vec<serde_json::Value>,
) {
    let target_display = display_path(root, &root.join(".fallowrc.json"));
    let workspace_relative = display_workspace_path(root, workspace_root);
    if !matches!(output, OutputFormat::Json) {
        let absolute = workspace_root.display();
        eprintln!(
            "Skipped duplicate-export config fix: no fallow config file at {} \
             and the directory is inside a monorepo (workspace root: {}). \
             Run `fallow init` at the workspace root, or invoke `fallow fix` \
             from {} instead of from a subpackage.",
            root.display(),
            absolute,
            absolute,
        );
    }
    fixes.push(serde_json::json!({
        "type": "add_ignore_exports",
        "config_key": "ignoreExports",
        "file": target_display,
        "skipped": true,
        "skip_reason": "monorepo_subpackage",
        "workspace_root": workspace_relative,
        "description": "Skipped: refusing to create .fallowrc.json inside a monorepo subpackage. Run `fallow init` at the workspace root.",
    }));
}

/// Render `workspace_root` relative to `root` (the analysis root) by
/// counting ancestor hops. Both paths are absolute in practice because
/// `workspace_root` was discovered by walking strictly upward from
/// `root` via `Path::parent`, so this just counts the number of
/// `parent()` steps from `root` to `workspace_root` and emits that many
/// `..` segments joined with `/`. Falls back to the absolute display
/// of `workspace_root` only when the ancestor walk cannot reach it
/// (cycle-guard tripped; cannot happen with real filesystem paths but
/// keeps the function total).
fn display_workspace_path(root: &Path, workspace_root: &Path) -> String {
    ancestor_distance(root, workspace_root).map_or_else(
        || workspace_root.display().to_string(),
        |depth| {
            if depth == 0 {
                ".".to_owned()
            } else {
                vec![".."; depth].join("/")
            }
        },
    )
}

/// Count ancestor hops from `start` to `ancestor`, or `None` if
/// `ancestor` is not on `start`'s ancestor chain. Guards against
/// unbounded walks with a fixed budget (real filesystem paths are
/// always shallow enough).
fn ancestor_distance(start: &Path, ancestor: &Path) -> Option<usize> {
    const MAX_DEPTH: usize = 256;
    let mut current = start;
    for depth in 0..MAX_DEPTH {
        if current == ancestor {
            return Some(depth);
        }
        current = current.parent()?;
    }
    None
}

fn emit_blocked_no_create(
    root: &Path,
    target: &Path,
    output: OutputFormat,
    fixes: &mut Vec<serde_json::Value>,
) {
    let target_display = display_path(root, target);
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Skipped duplicate-export config fix: no fallow config file at {} \
             and --no-create-config was passed. Either re-run `fallow fix` \
             without --no-create-config, or run `fallow init` first.",
            root.display()
        );
    }
    fixes.push(serde_json::json!({
        "type": "add_ignore_exports",
        "config_key": "ignoreExports",
        "file": target_display,
        "skipped": true,
        "skip_reason": "no_create_config",
        "description": "Skipped: --no-create-config was passed and no fallow config file exists.",
    }));
}

/// Render a `+`-prefix preview of a new file's content.
///
/// Used for the create-fallback dry-run. Hand-rolled to keep the dependency
/// surface small for the common case (the BEFORE side is always empty).
fn render_create_diff(path_display: &str, proposed: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "--- {path_display} (does not exist)");
    let _ = writeln!(out, "+++ {path_display} (proposed)");
    let line_count = proposed.lines().count();
    let _ = writeln!(out, "@@ -0,0 +1,{line_count} @@");
    for line in proposed.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Render a unified diff between current and proposed file contents.
fn render_unified_diff(path_display: &str, current: &str, proposed: &str) -> String {
    let diff = similar::TextDiff::from_lines(current, proposed);
    let mut out = String::new();
    let _ = writeln!(out, "--- {path_display} (current)");
    let _ = writeln!(out, "+++ {path_display} (proposed)");
    let unified = diff.unified_diff().context_radius(3).to_string();
    out.push_str(&unified);
    out
}

fn ignore_export_entries(
    root: &Path,
    config_path: &Path,
    duplicate_exports: &[DuplicateExportFinding],
) -> Vec<IgnoreExportRule> {
    let config_dir = config_path.parent().unwrap_or(root);
    let mut seen = FxHashSet::default();
    let mut entries = Vec::new();
    for item in duplicate_exports {
        let item = &item.export;
        for location in &item.locations {
            let file = relative_from_config_dir(root, config_dir, &location.path);
            if seen.insert(file.clone()) {
                entries.push(IgnoreExportRule {
                    file,
                    exports: vec!["*".to_owned()],
                });
            }
        }
    }
    entries
}

fn relative_from_config_dir(root: &Path, config_dir: &Path, file_path: &Path) -> String {
    let root_relative = file_path.strip_prefix(root).unwrap_or(file_path);
    let config_relative = config_dir
        .strip_prefix(root)
        .unwrap_or_else(|_| Path::new(""));
    lexical_relative(config_relative, root_relative)
        .unwrap_or_else(|| root_relative.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn lexical_relative(from_dir: &Path, to_file: &Path) -> Option<PathBuf> {
    let from = normal_components(from_dir)?;
    let to = normal_components(to_file)?;
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component);
    }
    Some(relative)
}

fn normal_components(path: &Path) -> Option<Vec<OsString>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => components.push(OsString::from("..")),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components)
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::results::{DuplicateExport, DuplicateLocation};

    fn duplicate(paths: &[PathBuf]) -> DuplicateExportFinding {
        DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "Button".to_owned(),
            locations: paths
                .iter()
                .map(|path| DuplicateLocation {
                    path: path.clone(),
                    line: 1,
                    col: 0,
                })
                .collect(),
        })
    }

    #[test]
    fn config_fix_reanchors_paths_to_workspace_config_dir() {
        let root = Path::new("/repo");
        let config_path = root.join("packages/ui/.fallowrc.json");
        let entries = ignore_export_entries(
            root,
            &config_path,
            &[duplicate(&[
                root.join("packages/ui/src/index.ts"),
                root.join("packages/shared/src/index.ts"),
            ])],
        );

        assert_eq!(entries[0].file, "src/index.ts");
        assert_eq!(entries[1].file, "../shared/src/index.ts");
    }

    #[test]
    fn config_fix_dedupes_exact_files_preserving_first_order() {
        let root = Path::new("/repo");
        let config_path = root.join(".fallowrc.json");
        let entries = ignore_export_entries(
            root,
            &config_path,
            &[duplicate(&[
                root.join("src/a.ts"),
                root.join("src/b.ts"),
                root.join("src/a.ts"),
            ])],
        );

        let files: Vec<&str> = entries.iter().map(|entry| entry.file.as_str()).collect();
        assert_eq!(files, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn create_diff_renders_addition_only_prefix() {
        let out = render_create_diff(".fallowrc.json", "{\n  \"a\": 1\n}\n");
        assert!(out.contains("--- .fallowrc.json (does not exist)"));
        assert!(out.contains("+++ .fallowrc.json (proposed)"));
        assert!(out.contains("+{"));
        assert!(out.contains("+  \"a\": 1"));
        assert!(out.contains("+}"));
        assert!(!out.contains("\n-"));
    }

    #[test]
    fn unified_diff_renders_additions_against_existing() {
        let current = "{\n  \"rules\": {}\n}\n";
        let proposed = "{\n  \"ignoreExports\": [\n    { \"file\": \"src/a.ts\", \"exports\": [\"*\"] }\n  ],\n  \"rules\": {}\n}\n";
        let diff = render_unified_diff(".fallowrc.json", current, proposed);
        assert!(diff.contains("--- .fallowrc.json (current)"));
        assert!(diff.contains("+++ .fallowrc.json (proposed)"));
        assert!(
            diff.lines()
                .any(|l| l.starts_with("+    { \"file\": \"src/a.ts\""))
        );
    }

    #[cfg(not(miri))]
    mod fs {
        use super::*;
        use fallow_types::results::AnalysisResults;

        fn results_with_duplicate(root: &Path, name: &str) -> AnalysisResults {
            AnalysisResults {
                duplicate_exports: vec![DuplicateExportFinding::with_actions(DuplicateExport {
                    export_name: name.to_owned(),
                    locations: vec![DuplicateLocation {
                        path: root.join("src/components/Button/index.ts"),
                        line: 1,
                        col: 0,
                    }],
                })],
                ..AnalysisResults::default()
            }
        }

        #[test]
        fn classify_returns_edit_when_config_exists() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join(".fallowrc.json"), "{}\n").unwrap();
            match classify_plan(root, None, false) {
                ResolvedConfigPlan::Edit { config_path } => {
                    assert!(config_path.ends_with(".fallowrc.json"));
                }
                other => panic!("expected Edit, got {other:?}"),
            }
        }

        #[test]
        fn classify_returns_create_when_no_config_and_no_workspace() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            match classify_plan(root, None, false) {
                ResolvedConfigPlan::Create { target } => {
                    assert_eq!(target, root.join(".fallowrc.json"));
                }
                other => panic!("expected Create, got {other:?}"),
            }
        }

        #[test]
        fn classify_returns_blocked_no_create_when_flag_set() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            match classify_plan(root, None, true) {
                ResolvedConfigPlan::BlockedNoCreate { target } => {
                    assert_eq!(target, root.join(".fallowrc.json"));
                }
                other => panic!("expected BlockedNoCreate, got {other:?}"),
            }
        }

        #[test]
        fn classify_returns_blocked_monorepo_for_pnpm_subpackage() {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path();
            std::fs::write(
                workspace.join("pnpm-workspace.yaml"),
                "packages:\n  - 'packages/*'\n",
            )
            .unwrap();
            let sub = workspace.join("packages/ui");
            std::fs::create_dir_all(&sub).unwrap();
            match classify_plan(&sub, None, false) {
                ResolvedConfigPlan::BlockedMonorepo { workspace_root } => {
                    assert_eq!(workspace_root, workspace);
                }
                other => panic!("expected BlockedMonorepo, got {other:?}"),
            }
        }

        #[test]
        fn classify_returns_blocked_monorepo_for_npm_workspaces_subpackage() {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path();
            std::fs::write(
                workspace.join("package.json"),
                r#"{"name":"root","workspaces":["packages/*"]}"#,
            )
            .unwrap();
            let sub = workspace.join("packages/api");
            std::fs::create_dir_all(&sub).unwrap();
            assert!(matches!(
                classify_plan(&sub, None, false),
                ResolvedConfigPlan::BlockedMonorepo { .. }
            ));
        }

        #[test]
        fn classify_returns_blocked_monorepo_for_turbo() {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path();
            std::fs::write(workspace.join("turbo.json"), "{}").unwrap();
            let sub = workspace.join("apps/web");
            std::fs::create_dir_all(&sub).unwrap();
            assert!(matches!(
                classify_plan(&sub, None, false),
                ResolvedConfigPlan::BlockedMonorepo { .. }
            ));
        }

        #[test]
        fn workspace_check_does_not_block_when_root_has_marker() {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path();
            std::fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n").unwrap();
            assert!(matches!(
                classify_plan(workspace, None, false),
                ResolvedConfigPlan::Create { .. }
            ));
        }

        #[test]
        fn dry_run_missing_config_writes_no_file_and_renders_diff() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            let err = apply_config_fixes(ConfigFixInput {
                    root,
                    config_path: None,
                    results: &results,
                    output: OutputFormat::Human,
                    dry_run: /* dry_run */ true,
                    no_create_config: /* no_create_config */ false,
                    fixes: &mut fixes,
            });
            assert!(!err);
            assert!(
                !root.join(".fallowrc.json").exists(),
                "dry-run must not write"
            );
            assert_eq!(fixes.len(), 1);
            let entry = &fixes[0];
            assert_eq!(entry["dry_run"], serde_json::json!(true));
            assert_eq!(
                entry["created_files"],
                serde_json::json!([".fallowrc.json"])
            );
            let diff = entry["proposed_diff"].as_str().expect("proposed_diff");
            assert!(diff.contains("--- .fallowrc.json (does not exist)"));
            assert!(diff.contains("\"ignoreExports\""));
        }

        #[test]
        fn apply_missing_config_creates_init_shape_file() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join("tsconfig.json"), "{}").unwrap();
            std::fs::create_dir_all(root.join(".storybook")).unwrap();
            std::fs::write(
                root.join("package.json"),
                r#"{"name":"app","devDependencies":{"vitest":"^1","react":"^18"}}"#,
            )
            .unwrap();

            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            let err = apply_config_fixes(ConfigFixInput {
                    root,
                    config_path: None,
                    results: &results,
                    output: OutputFormat::Human,
                    dry_run: /* dry_run */ false,
                    no_create_config: /* no_create_config */ false,
                    fixes: &mut fixes,
            });
            assert!(!err);
            assert_eq!(fixes.len(), 1);
            assert_eq!(fixes[0]["applied"], serde_json::json!(true));
            assert_eq!(
                fixes[0]["created_files"],
                serde_json::json!([".fallowrc.json"])
            );

            let path = root.join(".fallowrc.json");
            assert!(path.exists());
            let content = std::fs::read_to_string(&path).unwrap();
            let parsed: serde_json::Value = jsonc_parser::parse_to_serde_value(
                &content,
                &jsonc_parser::ParseOptions::default(),
            )
            .expect("seed parses as JSONC");
            assert!(parsed["$schema"].is_string(), "seed includes $schema");
            assert!(parsed["entry"].is_array(), "seed includes entry");
            assert!(
                parsed["ignorePatterns"]
                    .as_array()
                    .is_some_and(|arr| arr.iter().any(|v| v == ".storybook/**")),
                "seed includes Storybook ignore pattern"
            );
            assert_eq!(
                parsed["rules"]["unused-dependencies"], "warn",
                "seed includes test-framework rule"
            );
            let entries = parsed["ignoreExports"].as_array().expect("ignoreExports");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["file"], "src/components/Button/index.ts");
        }

        #[test]
        fn apply_missing_config_with_no_create_flag_refuses() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            let err = apply_config_fixes(ConfigFixInput {
                    root,
                    config_path: None,
                    results: &results,
                    output: OutputFormat::Human,
                    dry_run: /* dry_run */ false,
                    no_create_config: /* no_create_config */ true,
                    fixes: &mut fixes,
            });
            assert!(!err);
            assert!(!root.join(".fallowrc.json").exists());
            assert_eq!(fixes.len(), 1);
            assert_eq!(fixes[0]["skipped"], serde_json::json!(true));
            assert_eq!(fixes[0]["skip_reason"], "no_create_config");
        }

        #[test]
        fn apply_missing_config_in_monorepo_subpackage_refuses() {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path();
            std::fs::write(
                workspace.join("pnpm-workspace.yaml"),
                "packages:\n  - 'packages/*'\n",
            )
            .unwrap();
            let sub = workspace.join("packages/ui");
            std::fs::create_dir_all(&sub).unwrap();
            let results = results_with_duplicate(&sub, "Card");
            let mut fixes = Vec::new();
            let err = apply_config_fixes(ConfigFixInput {
                    root: &sub,
                    config_path: None,
                    results: &results,
                    output: OutputFormat::Human,
                    dry_run: /* dry_run */ false,
                    no_create_config: /* no_create_config */ false,
                    fixes: &mut fixes,
            });
            assert!(!err);
            assert!(!sub.join(".fallowrc.json").exists());
            assert_eq!(fixes.len(), 1);
            assert_eq!(fixes[0]["skipped"], serde_json::json!(true));
            assert_eq!(fixes[0]["skip_reason"], "monorepo_subpackage");
            assert_eq!(fixes[0]["workspace_root"], "../..");
        }

        #[test]
        fn dry_run_existing_jsonc_renders_diff_and_does_not_write() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let cfg_path = root.join(".fallowrc.jsonc");
            std::fs::write(&cfg_path, "{\n  // keep this comment\n  \"rules\": {}\n}\n").unwrap();
            let before = std::fs::read_to_string(&cfg_path).unwrap();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            apply_config_fixes(ConfigFixInput {
                root,
                config_path: None,
                results: &results,
                output: OutputFormat::Human,
                dry_run: true,
                no_create_config: false,
                fixes: &mut fixes,
            });
            assert_eq!(
                std::fs::read_to_string(&cfg_path).unwrap(),
                before,
                "dry-run must not modify the file"
            );
            assert_eq!(fixes.len(), 1);
            let diff = fixes[0]["proposed_diff"].as_str().unwrap();
            assert!(diff.contains("(current)") && diff.contains("(proposed)"));
            assert!(diff.contains("ignoreExports"));
        }

        #[test]
        fn dry_run_existing_toml_renders_diff() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let cfg_path = root.join("fallow.toml");
            std::fs::write(&cfg_path, "production = true\n").unwrap();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            apply_config_fixes(ConfigFixInput {
                root,
                config_path: None,
                results: &results,
                output: OutputFormat::Human,
                dry_run: true,
                no_create_config: false,
                fixes: &mut fixes,
            });
            assert_eq!(
                std::fs::read_to_string(&cfg_path).unwrap(),
                "production = true\n"
            );
            assert_eq!(fixes.len(), 1);
            let diff = fixes[0]["proposed_diff"].as_str().unwrap();
            assert!(diff.contains("[[ignoreExports]]"));
        }

        #[test]
        fn dry_run_existing_dot_fallow_toml_renders_diff() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let cfg_path = root.join(".fallow.toml");
            std::fs::write(&cfg_path, "").unwrap();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            apply_config_fixes(ConfigFixInput {
                root,
                config_path: None,
                results: &results,
                output: OutputFormat::Human,
                dry_run: true,
                no_create_config: false,
                fixes: &mut fixes,
            });
            assert_eq!(fixes.len(), 1);
            let diff = fixes[0]["proposed_diff"].as_str().unwrap();
            assert!(diff.contains("[[ignoreExports]]"));
        }

        #[test]
        fn dry_run_existing_json_renders_diff() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let cfg_path = root.join(".fallowrc.json");
            std::fs::write(&cfg_path, "{\n}\n").unwrap();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            apply_config_fixes(ConfigFixInput {
                root,
                config_path: None,
                results: &results,
                output: OutputFormat::Human,
                dry_run: true,
                no_create_config: false,
                fixes: &mut fixes,
            });
            assert_eq!(fixes.len(), 1);
            let diff = fixes[0]["proposed_diff"].as_str().unwrap();
            assert!(diff.contains("ignoreExports"));
            assert!(diff.contains("(current)"));
        }

        #[test]
        fn json_dry_run_includes_proposed_diff_field() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let results = results_with_duplicate(root, "Card");
            let mut fixes = Vec::new();
            apply_config_fixes(ConfigFixInput {
                root,
                config_path: None,
                results: &results,
                output: OutputFormat::Json,
                dry_run: true,
                no_create_config: false,
                fixes: &mut fixes,
            });
            assert_eq!(fixes.len(), 1);
            assert!(fixes[0]["proposed_diff"].is_string());
        }

        #[test]
        fn is_config_fixable_true_when_config_exists() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(".fallowrc.json"), "{}\n").unwrap();
            assert!(fallow_config::is_config_fixable(dir.path(), None));
        }

        #[test]
        fn is_config_fixable_true_when_can_create_at_root() {
            let dir = tempfile::tempdir().unwrap();
            assert!(fallow_config::is_config_fixable(dir.path(), None));
        }

        #[test]
        fn is_config_fixable_false_when_monorepo_subpackage() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n").unwrap();
            let sub = dir.path().join("packages/ui");
            std::fs::create_dir_all(&sub).unwrap();
            assert!(!fallow_config::is_config_fixable(&sub, None));
        }
    }
}
