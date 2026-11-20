//! Config actions for unused exports that a file edit cannot fix well.
//!
//! - A generated file: its generator writes the file again, so a removed
//!   export comes back. The finding keeps its `remove-export` action, but
//!   that action is not auto-fixable, and the inline suppression is dropped,
//!   because the generator removes the comment too.
//! - A shadcn/ui component folder (the `ui` alias of `components.json`): the
//!   files are copied as complete sets, and many projects keep each named
//!   part before it has a consumer.
//!
//! Each such finding gets an `add-to-config` action with the `ignoreExports`
//! rule that hides it. The findings stay in the report. The human report
//! prints one hint per group from these actions.

use std::io::Read;
use std::path::{Path, PathBuf};

use fallow_config::ResolvedConfig;
use fallow_types::output::{
    AddToConfigAction, AddToConfigKind, AddToConfigValue, GENERATED_FILE_EXPORTS_HINT,
    IgnoreExportsRule, IssueAction, UI_FOLDER_EXPORTS_HINT,
};
use fallow_types::output_dead_code::IGNORE_EXPORTS_VALUE_SCHEMA;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashMap;
use serde_json::Value;

/// The header length that the generated-file check needs.
const HEADER_BYTES: u64 = 4_096;

const COMPONENTS_FILE: &str = "components.json";

/// The tsconfig files that can hold the path alias of the `ui` folder.
const TSCONFIG_FILES: &[&str] = &["tsconfig.json", "tsconfig.app.json", "jsconfig.json"];

/// Add the config action to each unused-export and unused-type finding in a
/// generated file, and to each unused-export finding in a shadcn/ui folder.
///
/// The pass can run more than one time on the same results, and another pass
/// can set the fix action of a finding between two runs. So each run sets the
/// actions of a generated file again, and adds a config action only when the
/// finding has none.
pub fn add_unused_export_config_actions(results: &mut AnalysisResults, config: &ResolvedConfig) {
    if results.unused_exports.is_empty() && results.unused_types.is_empty() {
        return;
    }
    let mut files = FileFacts {
        root: config.root.as_path(),
        generated: FxHashMap::default(),
        ui_folders: FxHashMap::default(),
    };
    for finding in &mut results.unused_exports {
        let path = finding.export.path.clone();
        if files.is_generated(&path) {
            mark_generated(&mut finding.actions, &files.relative(&path));
        } else if !has_ignore_exports_action(&finding.actions)
            && let Some(folder) = files.ui_folder(&path)
        {
            let pattern = format!("{}/**", forward_slashes(&folder));
            finding
                .actions
                .push(ignore_exports_action(UI_FOLDER_EXPORTS_HINT, pattern));
        }
    }
    for finding in &mut results.unused_types {
        let path = finding.export.path.clone();
        if files.is_generated(&path) {
            mark_generated(&mut finding.actions, &files.relative(&path));
        }
    }
}

/// What the pass knows about each file and folder, read one time.
struct FileFacts<'a> {
    root: &'a Path,
    generated: FxHashMap<PathBuf, bool>,
    ui_folders: FxHashMap<PathBuf, Option<PathBuf>>,
}

impl FileFacts<'_> {
    fn relative(&self, path: &Path) -> String {
        forward_slashes(path.strip_prefix(self.root).unwrap_or(path))
    }

    fn is_generated(&mut self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(self.root) else {
            return false;
        };
        *self
            .generated
            .entry(path.to_path_buf())
            .or_insert_with(|| is_generated_file(relative, path))
    }

    fn ui_folder(&mut self, path: &Path) -> Option<PathBuf> {
        ui_folder_of(self.root, path, &mut self.ui_folders)
    }
}

fn has_ignore_exports_action(actions: &[IssueAction]) -> bool {
    actions.iter().any(
        |action| matches!(action, IssueAction::AddToConfig(add) if add.config_key == "ignoreExports"),
    )
}

/// Set the actions of a finding in a generated file: the code fix is not
/// auto-fixable, the inline suppression is gone, and the config action is
/// present one time.
fn mark_generated(actions: &mut Vec<IssueAction>, file: &str) {
    actions.retain(|action| !matches!(action, IssueAction::SuppressLine(_)));
    for action in actions.iter_mut() {
        if let IssueAction::Fix(fix) = action {
            fix.auto_fixable = false;
            fix.note = Some(GENERATED_FILE_EXPORTS_HINT.to_string());
        }
    }
    if !has_ignore_exports_action(actions) {
        actions.push(ignore_exports_action(
            GENERATED_FILE_EXPORTS_HINT,
            file.to_string(),
        ));
    }
}

fn ignore_exports_action(description: &str, file: String) -> IssueAction {
    IssueAction::AddToConfig(AddToConfigAction {
        kind: AddToConfigKind::AddToConfig,
        auto_fixable: false,
        description: description.to_string(),
        config_key: "ignoreExports".to_string(),
        value: AddToConfigValue::ExportsRules(vec![IgnoreExportsRule {
            file,
            exports: vec!["*".to_string()],
        }]),
        value_schema: Some(IGNORE_EXPORTS_VALUE_SCHEMA.to_string()),
    })
}

fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Whether the file is generated code, from its path below the project root
/// and from the start of its text.
fn is_generated_file(relative: &Path, path: &Path) -> bool {
    let mut header = String::new();
    let read = std::fs::File::open(path)
        .map(|file| file.take(HEADER_BYTES))
        .and_then(|mut head| {
            let mut bytes = Vec::new();
            head.read_to_end(&mut bytes).map(|_| bytes)
        });
    if let Ok(bytes) = read {
        header = String::from_utf8_lossy(&bytes).into_owned();
    }
    crate::source::is_generated_file(relative, &header)
}

/// The shadcn/ui folder that holds `path`, relative to the project root.
///
/// The nearest `components.json` at or above the file, up to the project
/// root, gives the folder. `folders` keeps the answer for each directory
/// that holds, or does not hold, such a file.
fn ui_folder_of(
    root: &Path,
    path: &Path,
    folders: &mut FxHashMap<PathBuf, Option<PathBuf>>,
) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if !dir.starts_with(root) {
            return None;
        }
        let folder = folders
            .entry(dir.to_path_buf())
            .or_insert_with(|| ui_folder_from_components_file(dir));
        if let Some(folder) = folder {
            return path
                .starts_with(&*folder)
                .then(|| folder.strip_prefix(root).ok().map(Path::to_path_buf))
                .flatten();
        }
        if dir == root {
            return None;
        }
        current = dir.parent();
    }
    None
}

/// The absolute `ui` folder of the `components.json` in `dir`, when the file
/// is a shadcn/ui config and its `ui` alias names an existing folder.
fn ui_folder_from_components_file(dir: &Path) -> Option<PathBuf> {
    let components = read_jsonc(&dir.join(COMPONENTS_FILE))?;
    alias_folder(dir, &ui_alias(&components)?)
}

/// The import path of the `ui` folder: `aliases.ui`, or `aliases.components`
/// with `/ui`. Only a file with the shadcn/ui schema URL counts.
fn ui_alias(components: &Value) -> Option<String> {
    let is_shadcn = components
        .get("$schema")
        .and_then(Value::as_str)
        .is_some_and(|schema| schema.contains("ui.shadcn.com"));
    if !is_shadcn {
        return None;
    }
    let aliases = components.get("aliases")?;
    let ui = aliases.get("ui").and_then(Value::as_str);
    let components = aliases.get("components").and_then(Value::as_str);
    ui.map(str::to_string)
        .or_else(|| components.map(|components| format!("{components}/ui")))
}

/// Resolve an import path such as `~/components/ui` to an existing folder,
/// through the `paths` of a tsconfig file in `dir`. Like TypeScript, the
/// pattern with the longest prefix wins. A pattern whose target is not a
/// folder gives way to the next pattern.
fn alias_folder(dir: &Path, alias: &str) -> Option<PathBuf> {
    TSCONFIG_FILES.iter().find_map(|name| {
        let tsconfig = read_jsonc(&dir.join(name))?;
        let options = tsconfig.get("compilerOptions")?;
        let base = options
            .get("baseUrl")
            .and_then(Value::as_str)
            .map_or_else(|| dir.to_path_buf(), |base| dir.join(base));
        let paths = options.get("paths")?.as_object()?;
        let mut candidates: Vec<(usize, PathBuf)> = paths
            .iter()
            .filter_map(|(pattern, targets)| {
                let prefix = pattern.strip_suffix('*')?;
                let rest = alias.strip_prefix(prefix)?;
                let target = targets.as_array()?.first()?.as_str()?;
                Some((
                    prefix.len(),
                    normalize(&base.join(target.replace('*', rest))),
                ))
            })
            .collect();
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        candidates
            .into_iter()
            .map(|(_, folder)| folder)
            .find(|folder| folder.is_dir())
    })
}

/// Remove the `.` and `..` components of a path without a file system read.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

fn read_jsonc(path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(path).ok()?;
    fallow_config::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_ui_alias_needs_the_shadcn_schema() {
        let shadcn = json!({
            "$schema": "https://ui.shadcn.com/schema.json",
            "aliases": { "components": "@/components", "ui": "~/components/ui" },
        });
        assert_eq!(ui_alias(&shadcn).as_deref(), Some("~/components/ui"));

        let without_ui = json!({
            "$schema": "https://ui.shadcn.com/schema.json",
            "aliases": { "components": "@/components" },
        });
        assert_eq!(ui_alias(&without_ui).as_deref(), Some("@/components/ui"));

        let other = json!({ "aliases": { "ui": "~/components/ui" } });
        assert_eq!(ui_alias(&other), None);
    }

    #[test]
    fn an_alias_resolves_through_the_longest_tsconfig_path_that_is_a_folder() {
        let project = tempfile::tempdir().expect("temp dir");
        let dir = project.path();
        std::fs::create_dir_all(dir.join("src/components/ui")).expect("ui folder");
        std::fs::write(
            dir.join("tsconfig.json"),
            r#"{
              // The app aliases.
              "compilerOptions": {
                "paths": { "@/*": ["./*"], "@/components/*": ["./src/components/*"] },
              },
            }"#,
        )
        .expect("tsconfig");

        assert_eq!(
            alias_folder(dir, "@/components/ui"),
            Some(dir.join("src/components/ui")),
            "the longer pattern wins"
        );
        assert_eq!(
            alias_folder(dir, "@/src/components/ui"),
            Some(dir.join("src/components/ui")),
            "the short pattern applies when no longer pattern matches"
        );
        assert_eq!(alias_folder(dir, "@workspace/ui/components"), None);
        assert_eq!(alias_folder(dir, "@/missing"), None);
    }

    fn remove_export_actions() -> Vec<IssueAction> {
        use fallow_types::output_dead_code::UnusedExportFinding;
        use fallow_types::results::UnusedExport;
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from("src/a.graphql.ts"),
            export_name: "operation".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
        .actions
    }

    #[test]
    fn a_generated_finding_stays_not_auto_fixable_after_another_pass_sets_the_fix() {
        let mut actions = remove_export_actions();
        mark_generated(&mut actions, "src/a.graphql.ts");
        assert_eq!(actions.len(), 2, "the fix and the config action");
        assert!(!actions.iter().any(IssueAction::is_auto_fixable));

        // The type-aware pass can set the fix action of the finding again.
        if let IssueAction::Fix(fix) = &mut actions[0] {
            fix.auto_fixable = true;
            fix.note = None;
        }
        mark_generated(&mut actions, "src/a.graphql.ts");

        assert_eq!(
            actions.len(),
            2,
            "a second run adds no second config action"
        );
        assert!(!actions.iter().any(IssueAction::is_auto_fixable));
    }
}
