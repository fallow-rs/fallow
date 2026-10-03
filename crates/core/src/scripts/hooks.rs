//! Git hook command scanner for dependency usage detection.
//!
//! Reads the commands that git hook managers and staged-file task runners
//! run: husky hook scripts, lefthook configs, and simple-git-hooks and
//! lint-staged configs, in package.json or in their own files. A package these
//! commands invoke is used, the same way a package.json script or a CI
//! workflow step makes it used.
//!
//! The scanner credits packages only. It does not seed entry files, because a
//! hook argument such as `{staged_files}` is a placeholder, not a path.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{IgnoredCommandEntries, ScriptCatalog, analyze_commands_with_context};

const LEFTHOOK_FILES: &[&str] = &[
    "lefthook.yml",
    "lefthook.yaml",
    ".lefthook.yml",
    ".lefthook.yaml",
    "lefthook-local.yml",
    "lefthook-local.yaml",
    ".lefthook-local.yml",
    ".lefthook-local.yaml",
];

const SIMPLE_GIT_HOOKS_JSON_FILES: &[&str] = &[".simple-git-hooks.json", "simple-git-hooks.json"];

const SIMPLE_GIT_HOOKS_JS_FILES: &[&str] = &[
    ".simple-git-hooks.js",
    ".simple-git-hooks.cjs",
    ".simple-git-hooks.mjs",
    "simple-git-hooks.js",
    "simple-git-hooks.cjs",
    "simple-git-hooks.mjs",
];

const LINT_STAGED_YAML_FILES: &[&str] = &[".lintstagedrc.yaml", ".lintstagedrc.yml"];

const LINT_STAGED_JS_FILES: &[&str] = &[
    ".lintstagedrc.js",
    ".lintstagedrc.cjs",
    ".lintstagedrc.mjs",
    ".lintstagedrc.ts",
    "lint-staged.config.js",
    "lint-staged.config.cjs",
    "lint-staged.config.mjs",
    "lint-staged.config.ts",
];

/// The package.json keys that hold hook or staged-file commands.
const MANIFEST_COMMAND_KEYS: &[&str] = &["simple-git-hooks", "lint-staged"];

/// Inputs shared by every hook source of one package root.
pub struct HookContext<'a> {
    pub bin_map: &'a FxHashMap<String, String>,
    pub declared_packages: &'a FxHashSet<String>,
    pub scripts: &'a ScriptCatalog,
    pub ignored: IgnoredCommandEntries<'a>,
}

/// Return the package names that the git hooks and staged-file commands
/// under `root` invoke.
#[must_use]
pub fn analyze_hook_files(root: &Path, context: &HookContext<'_>) -> FxHashSet<String> {
    let _span = tracing::info_span!("analyze_hook_files").entered();
    let commands = collect_hook_commands(root);
    if commands.is_empty() {
        return FxHashSet::default();
    }
    analyze_commands_with_context(
        &commands,
        root,
        context.bin_map,
        context.declared_packages,
        context.scripts,
        context.ignored,
    )
    .used_packages
}

/// Gather every hook and staged-file command string under `root`.
fn collect_hook_commands(root: &Path) -> Vec<String> {
    let mut commands = Vec::new();
    collect_husky_commands(root, &mut commands);

    for name in LEFTHOOK_FILES {
        if let Ok(content) = std::fs::read_to_string(root.join(name)) {
            commands.extend(super::ci::extract_ci_commands(&content));
        }
    }

    if let Some(manifest) = read_json(&root.join("package.json")) {
        for key in MANIFEST_COMMAND_KEYS {
            if let Some(value) = manifest.get(*key) {
                collect_json_strings(value, &mut commands);
            }
        }
    }

    for name in SIMPLE_GIT_HOOKS_JSON_FILES
        .iter()
        .chain(&[".lintstagedrc.json"])
    {
        if let Some(value) = read_json(&root.join(name)) {
            collect_json_strings(&value, &mut commands);
        }
    }

    // `.lintstagedrc` is JSON or YAML.
    if let Ok(content) = std::fs::read_to_string(root.join(".lintstagedrc")) {
        match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(value) => collect_json_strings(&value, &mut commands),
            Err(_) => commands.extend(yaml_scalar_values(&content)),
        }
    }
    for name in LINT_STAGED_YAML_FILES {
        if let Ok(content) = std::fs::read_to_string(root.join(name)) {
            commands.extend(yaml_scalar_values(&content));
        }
    }

    for name in SIMPLE_GIT_HOOKS_JS_FILES.iter().chain(LINT_STAGED_JS_FILES) {
        if let Ok(content) = std::fs::read_to_string(root.join(name)) {
            commands.extend(js_string_literals(&content));
        }
    }

    commands
}

/// Read the hook scripts directly inside `.husky/`. The `_` directory holds
/// husky's own runtime and is skipped with every other directory.
fn collect_husky_commands(root: &Path, commands: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(".husky")) else {
        return;
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
        })
        .collect();
    paths.sort();
    for path in paths {
        if let Ok(content) = std::fs::read_to_string(&path) {
            commands.extend(shell_script_commands(&content));
        }
    }
}

/// Split a shell script into command lines: comments, the shebang and the
/// `. "$(dirname -- "$0")/_/husky.sh"` sourcing line are skipped, and a line
/// that ends with `\` continues on the next one.
fn shell_script_commands(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut pending = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if pending.is_empty()
            && (trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(". "))
        {
            continue;
        }
        if let Some(continued) = trimmed.strip_suffix('\\') {
            pending.push_str(continued);
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        commands.push(std::mem::take(&mut pending));
    }
    if !pending.trim().is_empty() {
        commands.push(pending);
    }
    commands
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Collect every string value (not key) of a JSON value.
fn collect_json_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_json_strings(item, out);
            }
        }
        _ => {}
    }
}

/// The scalar values of a flat YAML mapping or list, as a lint-staged YAML
/// config writes them: `"*.ts": tool --fix` and `  - tool --check`.
fn yaml_scalar_values(content: &str) -> Vec<String> {
    let mut values = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let value = if let Some(item) = trimmed.strip_prefix("- ") {
            item
        } else if let Some((_, value)) = split_yaml_key(trimmed) {
            value
        } else {
            continue;
        };
        let value = unquote(value.trim());
        if !value.is_empty() && value != "|" && value != ">" {
            values.push(value.to_string());
        }
    }
    values
}

/// Split `key: value` where the key may be quoted and contain `:`.
fn split_yaml_key(line: &str) -> Option<(&str, &str)> {
    let first = line.chars().next()?;
    if first == '"' || first == '\'' {
        let close = line[1..].find(first)? + 1;
        let rest = line[close + 1..].trim_start();
        return rest.strip_prefix(':').map(|value| (&line[..=close], value));
    }
    line.split_once(": ")
        .or_else(|| line.strip_suffix(':').map(|key| (key, "")))
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// The string literals of a JavaScript config, as candidate commands. A
/// template literal contributes the text before its first `${`, which names
/// the binary in a command such as `` `tool ${files.join(" ")}` ``. Comments
/// are skipped. Strings that are not commands, such as glob keys, resolve to
/// no declared package and credit nothing.
fn js_string_literals(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index += 2;
            }
            quote @ (b'"' | b'\'' | b'`') => {
                let start = index + 1;
                let mut end = start;
                let mut template_cut = None;
                while end < bytes.len() && bytes[end] != quote {
                    if bytes[end] == b'\\' {
                        end += 1;
                    } else if quote == b'`'
                        && template_cut.is_none()
                        && bytes[end] == b'$'
                        && bytes.get(end + 1) == Some(&b'{')
                    {
                        template_cut = Some(end);
                    } else if bytes[end] == b'\n' && quote != b'`' {
                        break;
                    }
                    end += 1;
                }
                let literal_end = template_cut.unwrap_or(end).min(bytes.len());
                if let Some(text) = content.get(start..literal_end) {
                    let text = text.trim();
                    if !text.is_empty() && !text.contains('\n') {
                        literals.push(text.to_string());
                    }
                }
                index = end + 1;
            }
            _ => index += 1,
        }
    }
    literals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, content: &str) {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(full, content).expect("write file");
    }

    fn used(root: &Path, declared: &[&str]) -> Vec<String> {
        let declared: FxHashSet<String> = declared.iter().map(|d| (*d).to_string()).collect();
        let catalog = ScriptCatalog::default();
        let mut packages: Vec<String> = analyze_hook_files(
            root,
            &HookContext {
                bin_map: &FxHashMap::default(),
                declared_packages: &declared,
                scripts: &catalog,
                ignored: IgnoredCommandEntries::NONE,
            },
        )
        .into_iter()
        .filter(|name| declared.contains(name))
        .collect();
        packages.sort();
        packages
    }

    #[test]
    fn husky_hook_scripts_credit_invoked_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            root,
            ".husky/pre-commit",
            "#!/usr/bin/env sh\n. \"$(dirname -- \"$0\")/_/husky.sh\"\n\nnpx lint-tool --staged\n",
        );
        write(
            root,
            ".husky/commit-msg",
            "npx --no -- msg-tool --edit \"$1\"\n",
        );
        write(root, ".husky/_/husky.sh", "other-tool\n");
        assert_eq!(
            used(root, &["lint-tool", "msg-tool", "other-tool"]),
            vec!["lint-tool".to_string(), "msg-tool".to_string()]
        );
    }

    #[test]
    fn lefthook_run_commands_credit_invoked_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            root,
            "lefthook.yml",
            "pre-commit:\n  commands:\n    lint:\n      glob: \"*.js\"\n      run: npx lint-tool {staged_files}\n",
        );
        assert_eq!(used(root, &["lint-tool"]), vec!["lint-tool".to_string()]);
    }

    #[test]
    fn manifest_hook_and_staged_commands_credit_invoked_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            root,
            "package.json",
            r#"{
              "simple-git-hooks": { "pre-commit": "npx staged-runner" },
              "lint-staged": { "*.ts": ["lint-tool --fix", "format-tool --write"] }
            }"#,
        );
        assert_eq!(
            used(
                root,
                &["staged-runner", "lint-tool", "format-tool", "unused-tool"]
            ),
            vec![
                "format-tool".to_string(),
                "lint-tool".to_string(),
                "staged-runner".to_string()
            ]
        );
    }

    #[test]
    fn lint_staged_files_credit_invoked_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            root,
            ".lintstagedrc",
            "\"*.js\": lint-tool\n\"*.css\":\n  - style-tool --fix\n",
        );
        write(
            root,
            "lint-staged.config.mjs",
            "// comment-tool is not a command\nexport default {\n  '*.ts': (files) => `type-tool ${files.join(' ')}`,\n  '*.md': 'doc-tool',\n};\n",
        );
        assert_eq!(
            used(
                root,
                &[
                    "lint-tool",
                    "style-tool",
                    "type-tool",
                    "doc-tool",
                    "comment-tool"
                ]
            ),
            vec![
                "doc-tool".to_string(),
                "lint-tool".to_string(),
                "style-tool".to_string(),
                "type-tool".to_string()
            ]
        );
    }

    #[test]
    fn no_hook_sources_credit_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            root,
            "package.json",
            r#"{ "scripts": { "lint": "lint-tool" } }"#,
        );
        assert!(used(root, &["lint-tool"]).is_empty());
    }
}
