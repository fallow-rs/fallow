//! Commands in project files that a tool runs at the project root.
//!
//! - A development container (`devcontainer.json`) runs its lifecycle
//!   commands when the container is created or started.
//! - A `t3.json` project file lists scripts that the T3 Code app runs.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The lifecycle keys of `devcontainer.json`. Each holds a command string,
/// an argument array, or an object whose values are one of those.
const DEVCONTAINER_COMMAND_KEYS: &[&str] = &[
    "initializeCommand",
    "onCreateCommand",
    "updateContentCommand",
    "postCreateCommand",
    "postStartCommand",
    "postAttachCommand",
];

const T3_PROJECT_FILE: &str = "t3.json";

/// The commands of each development container file and of `t3.json`, with
/// the path of the file relative to the project root.
pub(super) fn read_command_files(root: &Path) -> Vec<(String, Vec<String>)> {
    let mut files = Vec::new();
    for path in devcontainer_files(root) {
        let Some(config) = read_jsonc(&path) else {
            continue;
        };
        let mut commands = Vec::new();
        for key in DEVCONTAINER_COMMAND_KEYS {
            if let Some(value) = config.get(key) {
                push_lifecycle_commands(value, &mut commands);
            }
        }
        push_file_commands(root, &path, commands, &mut files);
    }
    let project_file = root.join(T3_PROJECT_FILE);
    if let Some(project) = read_jsonc(&project_file) {
        let mut commands = Vec::new();
        push_t3_script_commands(&project, &mut commands);
        push_file_commands(root, &project_file, commands, &mut files);
    }
    files
}

fn push_file_commands(
    root: &Path,
    path: &Path,
    commands: Vec<String>,
    files: &mut Vec<(String, Vec<String>)>,
) {
    if commands.is_empty() {
        return;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    files.push((relative.to_string_lossy().replace('\\', "/"), commands));
}

/// The locations that the development container specification defines:
/// `.devcontainer.json`, `.devcontainer/devcontainer.json`, and one
/// `devcontainer.json` in each folder directly below `.devcontainer/`.
fn devcontainer_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(".devcontainer");
    let mut files = vec![
        root.join(".devcontainer.json"),
        dir.join("devcontainer.json"),
    ];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut nested: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path().join("devcontainer.json"))
            .filter(|path| path.is_file())
            .collect();
        nested.sort();
        files.extend(nested);
    }
    files
}

fn read_jsonc(path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(path).ok()?;
    fallow_config::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()
}

fn push_lifecycle_commands(value: &Value, commands: &mut Vec<String>) {
    match value {
        Value::Object(named) => {
            for command in named.values() {
                commands.extend(lifecycle_command(command));
            }
        }
        _ => commands.extend(lifecycle_command(value)),
    }
}

/// A command string, or the words of an argument array (`["node", "x.mjs"]`).
/// The container runs an array without a shell, so each word is quoted when
/// the shell parser would split it or read an operator in it.
fn lifecycle_command(value: &Value) -> Option<String> {
    match value {
        Value::String(command) => Some(command.clone()),
        Value::Array(words) => {
            let words: Vec<String> = words
                .iter()
                .filter_map(Value::as_str)
                .map(shell_word)
                .collect();
            (!words.is_empty()).then(|| words.join(" "))
        }
        _ => None,
    }
}

/// `word` as one shell word: unchanged when it holds only plain characters,
/// in single quotes otherwise.
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_./:=@%+,-".contains(&byte));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

fn push_t3_script_commands(project: &Value, commands: &mut Vec<String>) {
    let Some(scripts) = project.get("scripts").and_then(Value::as_array) else {
        return;
    };
    commands.extend(
        scripts
            .iter()
            .filter_map(|script| script.get("command").and_then(Value::as_str))
            .map(str::to_string),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lifecycle(value: &Value) -> Vec<String> {
        let mut commands = Vec::new();
        push_lifecycle_commands(value, &mut commands);
        commands
    }

    #[test]
    fn a_lifecycle_command_is_a_string_an_array_or_an_object_of_those() {
        assert_eq!(lifecycle(&json!("bash setup.sh")), ["bash setup.sh"]);
        assert_eq!(lifecycle(&json!(["node", "a.mjs"])), ["node a.mjs"]);
        assert_eq!(
            lifecycle(&json!({ "a": "node a.mjs", "b": ["node", "b.mjs"], "c": 1 })),
            ["node a.mjs", "node b.mjs"]
        );
        assert!(lifecycle(&json!(null)).is_empty());
    }

    #[test]
    fn an_array_word_with_a_space_or_an_operator_stays_one_word() {
        assert_eq!(
            lifecycle(&json!(["node", "my scripts/a b.mjs"])),
            ["node 'my scripts/a b.mjs'"]
        );
        assert_eq!(
            lifecycle(&json!(["sh", "-c", "tsc && echo it's"])),
            [r"sh -c 'tsc && echo it'\''s'"]
        );
    }

    #[test]
    fn each_file_gives_its_path_from_the_project_root() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir_all(root.join(".devcontainer/api")).expect("devcontainer dir");
        std::fs::write(
            root.join(".devcontainer/api/devcontainer.json"),
            r#"{ "postCreateCommand": "node scripts/api.mjs" }"#,
        )
        .expect("devcontainer file");
        std::fs::write(root.join(".devcontainer.json"), r#"{ "image": "node:24" }"#)
            .expect("devcontainer file without a command");
        std::fs::write(
            root.join("t3.json"),
            r#"{ "scripts": [{ "command": "node scripts/setup.mjs" }] }"#,
        )
        .expect("project file");

        assert_eq!(
            read_command_files(root),
            [
                (
                    ".devcontainer/api/devcontainer.json".to_string(),
                    vec!["node scripts/api.mjs".to_string()]
                ),
                (
                    "t3.json".to_string(),
                    vec!["node scripts/setup.mjs".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn t3_scripts_give_their_commands() {
        let mut commands = Vec::new();
        push_t3_script_commands(
            &json!({ "scripts": [{ "name": "Dev", "command": "vp run dev" }, { "name": "x" }] }),
            &mut commands,
        );
        assert_eq!(commands, ["vp run dev"]);
    }
}
