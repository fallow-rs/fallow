//! TypeScript content mapper plugin.
//!
//! The `typescript.contentMapper` key of `package.json` tells TypeScript to
//! start a mapper process for non-TypeScript content. `exec` is the command
//! line of that process, as an argument array. TypeScript runs the command, so
//! no code imports its script. The plugin reads the command like a package
//! script: the script file becomes an entry point and the binary credits its
//! package.

use std::path::Path;

use fallow_config::{EntryPointRole, PackageJson};
use rustc_hash::FxHashMap;

use super::{Plugin, PluginResult};
use crate::scripts;

pub struct TypescriptContentMapperPlugin;

impl Plugin for TypescriptContentMapperPlugin {
    fn name(&self) -> &'static str {
        "typescript-content-mapper"
    }

    fn entry_point_role(&self) -> EntryPointRole {
        EntryPointRole::Runtime
    }

    fn is_enabled_with_package_json(&self, pkg: &PackageJson, _root: &Path) -> bool {
        content_mapper(pkg).is_some_and(is_truthy)
    }

    fn resolve_package_json(&self, pkg: &PackageJson, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(command) = content_mapper(pkg).and_then(exec_command) else {
            return result;
        };

        let analysis = scripts::analyze_command(&command, root, &FxHashMap::default());
        result
            .referenced_dependencies
            .extend(analysis.used_packages);
        result.extend_entry_patterns(
            analysis
                .entry_files
                .iter()
                .map(|file| file.trim_start_matches("./").to_string()),
        );
        result
    }
}

fn content_mapper(pkg: &PackageJson) -> Option<&serde_json::Value> {
    pkg.typescript.as_ref()?.get("contentMapper")
}

fn is_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

/// The `exec` argument array as one shell command. Each argument is quoted,
/// so an argument with a space stays one word. An `exec` value that is not an
/// array of strings gives no command.
fn exec_command(mapper: &serde_json::Value) -> Option<String> {
    let args = mapper.get("exec")?.as_array()?;
    let words: Option<Vec<String>> = args
        .iter()
        .map(|arg| {
            arg.as_str()
                .map(|arg| format!("'{}'", arg.replace('\'', r"'\''")))
        })
        .collect();
    let words = words?;
    (!words.is_empty()).then(|| words.join(" "))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn exec_array_becomes_quoted_command() {
        let mapper = json!({ "exec": ["node", "lib/transform server.js"] });
        assert_eq!(
            exec_command(&mapper).as_deref(),
            Some("'node' 'lib/transform server.js'")
        );
    }

    #[test]
    fn exec_with_non_string_gives_no_command() {
        assert!(exec_command(&json!({ "exec": ["node", 1] })).is_none());
        assert!(exec_command(&json!({ "exec": "node lib/x.js" })).is_none());
        assert!(exec_command(&json!({ "exec": [] })).is_none());
        assert!(exec_command(&json!({})).is_none());
    }

    #[test]
    fn quoted_command_gives_entry_file() {
        let command =
            exec_command(&json!({ "exec": ["node", "lib/transform-server.js"] })).expect("command");
        let analysis =
            scripts::analyze_command(&command, Path::new("/project"), &FxHashMap::default());
        assert_eq!(analysis.entry_files, vec!["lib/transform-server.js"]);
    }

    #[test]
    fn falsy_mapper_does_not_enable() {
        assert!(!is_truthy(&json!(false)));
        assert!(!is_truthy(&json!(null)));
        assert!(is_truthy(&json!({ "exec": [] })));
    }

    #[test]
    fn entry_point_role_is_runtime() {
        assert_eq!(
            TypescriptContentMapperPlugin.entry_point_role(),
            EntryPointRole::Runtime
        );
    }
}
