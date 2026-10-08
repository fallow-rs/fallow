//! Commands of Nx `run-commands` targets.
//!
//! An Nx project can run a command through a `project.json` target instead of
//! a `package.json` script. Fallow reads those commands as scripts, so a
//! `--config` file, an entry file, or a binary in them is credited the same
//! way.

use rustc_hash::FxHashMap;
use serde_json::Value;

use super::shell;

/// Executors that run a shell command. Other executors name a package, not a
/// command.
const RUN_COMMANDS_EXECUTORS: &[&str] = &[
    "nx:run-commands",
    "@nx/workspace:run-commands",
    "@nrwl/workspace:run-commands",
    "nx:run-script",
];

/// Longest chain of targets that call each other.
const MAX_INLINE_DEPTH: usize = 4;

/// The commands that the `run-commands` targets of a `project.json` run, keyed
/// by target name. A configuration that sets its own command adds
/// `target:configuration`.
///
/// `project_root` is the project directory relative to the workspace root, or
/// empty for a root project. Nx runs a command from `cwd`, which is the
/// workspace root when unset. Fallow resolves the paths of a command against
/// the project directory, so a target that runs from another directory gives no
/// script: a path in it would point at the wrong file.
#[must_use]
pub fn run_commands_scripts(project_json: &str, project_root: &str) -> FxHashMap<String, String> {
    let Ok(project) = serde_json::from_str::<Value>(project_json) else {
        return FxHashMap::default();
    };
    let Some(targets) = project.get("targets").and_then(Value::as_object) else {
        return FxHashMap::default();
    };

    let mut commands: FxHashMap<String, String> = FxHashMap::default();
    for (name, target) in targets {
        if !is_run_commands(target) {
            continue;
        }
        let options = target.get("options");
        if let Some(command) = target_command(options, options, project_root) {
            commands.insert(name.clone(), command);
        }
        let Some(configurations) = target.get("configurations").and_then(Value::as_object) else {
            continue;
        };
        for (configuration, overrides) in configurations {
            if overrides.get("command").is_none() && overrides.get("commands").is_none() {
                continue;
            }
            if let Some(command) = target_command(Some(overrides), options, project_root) {
                commands.insert(format!("{name}:{configuration}"), command);
            }
        }
    }

    let targets = commands.clone();
    commands
        .into_iter()
        .map(|(name, command)| (name, inline_targets(&command, &targets, MAX_INLINE_DEPTH)))
        .collect()
}

fn is_run_commands(target: &Value) -> bool {
    target
        .get("executor")
        .and_then(Value::as_str)
        .is_some_and(|executor| RUN_COMMANDS_EXECUTORS.contains(&executor))
}

/// One target (or configuration) as a single command line, or `None` when it
/// has no command or runs from another directory. `own` holds the command and
/// may hold a `cwd`; `base` holds the `cwd` that `own` inherits.
fn target_command(own: Option<&Value>, base: Option<&Value>, project_root: &str) -> Option<String> {
    let cwd = own
        .and_then(|options| options.get("cwd"))
        .or_else(|| base.and_then(|options| options.get("cwd")))
        .and_then(Value::as_str);
    if !runs_in_project(cwd, project_root) {
        return None;
    }
    let own = own?;
    let parts: Vec<String> = match (own.get("commands"), own.get("command")) {
        (Some(Value::Array(commands)), _) => commands.iter().filter_map(command_text).collect(),
        (_, Some(Value::String(command))) => vec![command.clone()],
        _ => Vec::new(),
    };
    if parts.is_empty() {
        return None;
    }
    Some(expand_tokens(&parts.join(" && "), project_root))
}

fn command_text(command: &Value) -> Option<String> {
    match command {
        Value::String(text) => Some(text.clone()),
        Value::Object(object) => object.get("command")?.as_str().map(str::to_string),
        _ => None,
    }
}

fn runs_in_project(cwd: Option<&str>, project_root: &str) -> bool {
    let Some(cwd) = cwd else {
        return project_root.is_empty();
    };
    let cwd = cwd.replace("{projectRoot}", project_root);
    let normalize = |path: &str| {
        path.trim_start_matches("./")
            .trim_end_matches('/')
            .trim_matches('.')
            .to_string()
    };
    normalize(&cwd) == normalize(project_root)
}

/// Replace the Nx path tokens. `{workspaceRoot}` becomes the way up from the
/// project directory, because a command runs from the project directory.
fn expand_tokens(command: &str, project_root: &str) -> String {
    let project = if project_root.is_empty() {
        "."
    } else {
        project_root
    };
    let up = if project_root.is_empty() {
        ".".to_string()
    } else {
        vec![
            "..";
            project_root
                .split('/')
                .filter(|part| !part.is_empty())
                .count()
        ]
        .join("/")
    };
    command
        .replace("{projectRoot}", project)
        .replace("{workspaceRoot}", &up)
}

/// Replace each `nx <target>` call to a target of the same project with the
/// command of that target and the arguments of the call.
fn inline_targets(command: &str, targets: &FxHashMap<String, String>, depth: usize) -> String {
    if depth == 0 {
        return command.to_string();
    }
    shell::split_shell_operators(command)
        .into_iter()
        .map(|segment| inline_segment(segment, targets, depth))
        .collect::<Vec<_>>()
        .join(" && ")
}

fn inline_segment(segment: &str, targets: &FxHashMap<String, String>, depth: usize) -> String {
    let words = shell::split_words(segment);
    let tokens: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
    let Some(first) = shell::skip_initial_wrappers(&tokens, 0) else {
        return segment.trim().to_string();
    };
    let nx_at = match tokens.get(first) {
        Some(&"nx") => first,
        Some(&"npx" | &"pnpx" | &"bunx") if tokens.get(first + 1) == Some(&"nx") => first + 1,
        _ => return segment.trim().to_string(),
    };
    let Some(target) = tokens
        .get(nx_at + 1)
        .filter(|name| targets.contains_key(**name))
    else {
        return segment.trim().to_string();
    };

    let mut rest_at = nx_at + 2;
    if tokens.get(rest_at) == Some(&"--") {
        rest_at += 1;
    }
    let head = &segment[..words[first].start];
    let runner = &segment[words[first].start..words[nx_at].start];
    let tail = words.get(rest_at).map_or("", |word| &segment[word.start..]);
    let inlined = inline_targets(&targets[*target], targets, depth - 1);
    format!("{head}{runner}{inlined} {tail}").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripts(json: &str, root: &str) -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> =
            run_commands_scripts(json, root).into_iter().collect();
        found.sort();
        found
    }

    #[test]
    fn reads_a_command_and_a_commands_array() {
        let json = r#"{"targets":{
            "test":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"jest --config=jest.config.mjs"}},
            "build":{"executor":"@nx/workspace:run-commands","options":{"cwd":"packages/app","commands":["rimraf dist",{"command":"tsc -p tsconfig.build.json"}]}}
        }}"#;
        assert_eq!(
            scripts(json, "packages/app"),
            vec![
                (
                    "build".to_string(),
                    "rimraf dist && tsc -p tsconfig.build.json".to_string()
                ),
                (
                    "test".to_string(),
                    "jest --config=jest.config.mjs".to_string()
                ),
            ]
        );
    }

    #[test]
    fn skips_other_executors_and_foreign_working_directories() {
        let json = r#"{"targets":{
            "build":{"executor":"@nx/vite:build","options":{"command":"vite build"}},
            "lint":{"executor":"nx:run-commands","options":{"command":"eslint ."}},
            "other":{"executor":"nx:run-commands","options":{"cwd":"packages/other","command":"tsx scripts/x.ts"}}
        }}"#;
        assert!(scripts(json, "packages/app").is_empty());
    }

    #[test]
    fn a_target_without_cwd_runs_from_the_workspace_root() {
        let json = r#"{"targets":{"lint":{"executor":"nx:run-commands","options":{"command":"eslint ."}}}}"#;
        assert_eq!(
            scripts(json, ""),
            vec![("lint".to_string(), "eslint .".to_string())]
        );
    }

    #[test]
    fn expands_tokens_for_the_project_and_the_workspace() {
        let json = r#"{"targets":{"gen":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"tsx {workspaceRoot}/tools/gen.ts {projectRoot}/schema.json"}}}}"#;
        assert_eq!(
            scripts(json, "packages/app"),
            vec![(
                "gen".to_string(),
                "tsx ../../tools/gen.ts packages/app/schema.json".to_string()
            )]
        );
    }

    #[test]
    fn a_configuration_can_replace_the_command() {
        let json = r#"{"targets":{"test":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"jest"},
            "configurations":{"ci":{"command":"jest --config jest.ci.config.ts --ci"}}}}}"#;
        let found = scripts(json, "packages/app");
        assert!(found.contains(&("test".to_string(), "jest".to_string())));
        assert!(found.contains(&(
            "test:ci".to_string(),
            "jest --config jest.ci.config.ts --ci".to_string()
        )));
    }

    #[test]
    fn inlines_a_call_to_another_target_of_the_same_project() {
        let json = r#"{"targets":{
            "jest":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"jest"}},
            "ts-node":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"ts-node"}},
            "integration":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","commands":["NODE_ENV=test NODE_OPTIONS=\"--max-old-space-size=6144\" nx jest --config ./jest-integration.config.ts --logHeapUsage"]}},
            "migrate":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"npx nx ts-node -- src/database/run.ts && echo done"}}
        }}"#;
        let found = scripts(json, "packages/app");
        assert!(found.contains(&(
            "integration".to_string(),
            "NODE_ENV=test NODE_OPTIONS=\"--max-old-space-size=6144\" jest --config ./jest-integration.config.ts --logHeapUsage".to_string()
        )));
        assert!(found.contains(&(
            "migrate".to_string(),
            "npx ts-node src/database/run.ts && echo done".to_string()
        )));
    }

    #[test]
    fn a_target_that_calls_itself_does_not_loop() {
        let json = r#"{"targets":{"a":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"nx b"}},
            "b":{"executor":"nx:run-commands","options":{"cwd":"{projectRoot}","command":"nx a"}}}}"#;
        let _ = scripts(json, "packages/app");
    }

    #[test]
    fn malformed_json_gives_no_scripts() {
        assert!(scripts("{ not json", "packages/app").is_empty());
    }
}
