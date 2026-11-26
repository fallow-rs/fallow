//! The directories of the apps that an end-to-end runner starts as its web
//! server, such as `app/` for a Playwright `webServer.command` of
//! `pnpm start` with a `start` script `cd ./app && pnpm run dev`.
//!
//! Entry discovery moves the runtime entry points below such a directory to
//! the support role when the directory is a nested package that is not a
//! workspace package. The app exists for the tests, so its files are not
//! production code for the coverage gap rules (issue #5270).

use std::path::{Path, PathBuf};

use super::config_parser;
use crate::scripts::{self, ScriptCatalog};

/// The maximum depth of package script calls that the search follows.
const MAX_SCRIPT_DEPTH: usize = 4;

/// Package manager options that set the directory of the command.
const DIRECTORY_OPTIONS: &[&str] = &["--dir", "-C", "--prefix", "--cwd"];

const PACKAGE_MANAGERS: &[&str] = &["npm", "pnpm", "yarn", "bun"];

/// The npm commands that run the package script of the same name without
/// `run`, such as `npm start`.
const NPM_SCRIPT_COMMANDS: &[&str] = &["start", "restart"];

/// Package manager commands and script names that prepare or check a
/// package but do not start a server. A folder where only such a command
/// runs is not an app folder: `pnpm -C tools/gen build && pnpm start`
/// starts the server in the config folder. A script name also matches with
/// a `:` suffix, such as `build:esm`.
const SETUP_COMMANDS: &[&str] = &[
    "install",
    "i",
    "ci",
    "add",
    "remove",
    "rm",
    "uninstall",
    "update",
    "up",
    "upgrade",
    "link",
    "unlink",
    "audit",
    "outdated",
    "publish",
    "pack",
    "prune",
    "dedupe",
    "rebuild",
    "build",
    "lint",
    "test",
    "typecheck",
    "format",
    "clean",
    "generate",
    "codegen",
];

/// Shell commands that start no server.
const SHELL_BUILTINS: &[&str] = &[
    "echo", "export", "set", "true", "false", "sleep", "wait", "rm", "mkdir", "cp", "mv", "touch",
];

/// The folders that a web server command starts an app in, below
/// `config_dir`.
///
/// `base` is the working directory of the command: the config folder, or
/// the `cwd` of the web server. The search follows `cd <dir>`, the
/// directory options of a package manager, and the package scripts that
/// the command runs. A folder counts when a command that can start a
/// server runs in it, and when it has its own `package.json`.
pub(super) fn entered_app_dirs(command: &str, base: &Path, config_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_dirs(command, base, 0, &mut dirs);
    let config_dir = config_parser::lexical_normalize(config_dir);
    let mut kept: Vec<PathBuf> = dirs
        .into_iter()
        .map(|dir| config_parser::lexical_normalize(&dir))
        .filter(|dir| dir != &config_dir && dir.starts_with(&config_dir))
        .filter(|dir| dir.join("package.json").is_file())
        .collect();
    kept.sort_unstable();
    kept.dedup();
    kept
}

fn collect_dirs(command: &str, cwd: &Path, depth: usize, dirs: &mut Vec<PathBuf>) {
    let mut cwd = cwd.to_path_buf();
    for segment in scripts::split_shell_operators(command) {
        let words: Vec<&str> = segment
            .split_whitespace()
            .map(|word| word.trim_matches(['"', '\'', '(', ')']))
            .filter(|word| !word.is_empty())
            .skip_while(|word| is_env_assignment(word))
            .collect();
        match words.as_slice() {
            [] => {}
            ["cd", target] => {
                let Some(dir) = relative_dir(&cwd, target) else {
                    return;
                };
                cwd = dir;
            }
            ["cd" | "pushd" | "popd", ..] => return,
            [binary, args @ ..] if PACKAGE_MANAGERS.contains(binary) => {
                let run_dir = directory_option(&cwd, args).unwrap_or_else(|| cwd.clone());
                let Some(name) = command_name(args) else {
                    continue;
                };
                if is_setup_command(name) {
                    continue;
                }
                dirs.push(run_dir.clone());
                if depth < MAX_SCRIPT_DEPTH {
                    let npm_script =
                        (*binary == "npm" && NPM_SCRIPT_COMMANDS.contains(&name)).then_some(name);
                    follow_scripts(segment, npm_script, &run_dir, depth, dirs);
                }
            }
            [binary, ..] if SHELL_BUILTINS.contains(binary) => {}
            _ => dirs.push(cwd.clone()),
        }
    }
}

/// A leading `NAME=value` word of a command.
fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

/// The script or command that a package manager runs: the word after `run`,
/// else the first word that is not an option or an option value.
fn command_name<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut positional = Vec::new();
    let mut skip_value = false;
    for arg in args {
        if skip_value {
            skip_value = false;
        } else if DIRECTORY_OPTIONS.contains(arg) {
            skip_value = true;
        } else if !arg.starts_with('-') {
            positional.push(*arg);
        }
    }
    match positional.as_slice() {
        ["run" | "run-script", name, ..] | [name, ..] => Some(name),
        [] => None,
    }
}

fn is_setup_command(name: &str) -> bool {
    let base = name.split_once(':').map_or(name, |(base, _)| base);
    SETUP_COMMANDS.contains(&base)
}

/// Follow the package scripts of the package in `dir` that `segment` runs.
/// `npm_script` is the script of an npm command such as `npm start`.
fn follow_scripts(
    segment: &str,
    npm_script: Option<&str>,
    dir: &Path,
    depth: usize,
    dirs: &mut Vec<PathBuf>,
) {
    let Some(scripts) = fallow_config::load_dir_package_json(dir).and_then(|pkg| pkg.scripts)
    else {
        return;
    };
    let catalog = ScriptCatalog::from_scripts(&scripts);
    let mut names: Vec<String> = scripts::referenced_package_scripts(segment, &catalog)
        .into_iter()
        .chain(npm_script.map(str::to_string))
        .collect();
    names.sort_unstable();
    names.dedup();
    for name in names {
        if let Some(body) = scripts.get(&name) {
            collect_dirs(body, dir, depth + 1, dirs);
        }
    }
}

/// The directory of a `--dir <dir>`, `--dir=<dir>` or `-C <dir>` option.
fn directory_option(cwd: &Path, args: &[&str]) -> Option<PathBuf> {
    args.iter().enumerate().find_map(|(idx, arg)| {
        if DIRECTORY_OPTIONS.contains(arg) {
            return relative_dir(cwd, args.get(idx + 1)?);
        }
        let (option, value) = arg.split_once('=')?;
        DIRECTORY_OPTIONS
            .contains(&option)
            .then(|| relative_dir(cwd, value))
            .flatten()
    })
}

/// `cwd` joined with a literal relative directory. `None` for an absolute,
/// home-relative or interpolated directory.
fn relative_dir(cwd: &Path, target: &str) -> Option<PathBuf> {
    let literal = !target.is_empty()
        && !target.starts_with(['-', '~', '/', '$'])
        && !target.contains(['$', '`', '*', '?']);
    literal.then(|| cwd.join(target.trim_start_matches("./")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, content) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, content).expect("write");
        }
        dir
    }

    fn relative(dirs: &[PathBuf], root: &Path) -> Vec<String> {
        dirs.iter()
            .map(|dir| {
                dir.strip_prefix(root)
                    .expect("below root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    const APP_MANIFEST: &str = r#"{ "scripts": { "dev": "vite" } }"#;

    #[test]
    fn follows_a_package_script_into_a_cd_directory() {
        let dir = project(&[
            (
                "package.json",
                r#"{ "scripts": { "start": "pnpm build && cd ./app && pnpm run dev" } }"#,
            ),
            ("app/package.json", APP_MANIFEST),
        ]);
        let root = dir.path();
        let dirs = entered_app_dirs("pnpm start", root, root);
        assert_eq!(relative(&dirs, root), ["app"]);
    }

    #[test]
    fn follows_the_npm_start_command() {
        let dir = project(&[
            (
                "package.json",
                r#"{ "scripts": { "start": "cd app && npm install && npm run dev" } }"#,
            ),
            ("app/package.json", APP_MANIFEST),
        ]);
        let root = dir.path();
        let dirs = entered_app_dirs("npm start", root, root);
        assert_eq!(relative(&dirs, root), ["app"]);
    }

    #[test]
    fn a_setup_command_does_not_mark_its_folder() {
        let dir = project(&[
            ("package.json", r#"{ "scripts": { "dev": "vite" } }"#),
            (
                "tools/gen/package.json",
                r#"{ "scripts": { "build": "tsc" } }"#,
            ),
            ("lib/package.json", r#"{ "scripts": { "build": "tsc" } }"#),
            ("app/package.json", APP_MANIFEST),
        ]);
        let root = dir.path();
        assert!(entered_app_dirs("pnpm -C tools/gen build && pnpm dev", root, root).is_empty());
        assert!(
            entered_app_dirs("npm --prefix lib run build:esm && npm run dev", root, root)
                .is_empty()
        );
        assert!(
            entered_app_dirs("cd app && npm install && cd .. && npm run dev", root, root)
                .is_empty()
        );
    }

    #[test]
    fn follows_a_sub_shell_and_an_env_assignment() {
        let dir = project(&[("app/package.json", APP_MANIFEST)]);
        let root = dir.path();
        assert_eq!(
            relative(
                &entered_app_dirs("(cd app && PORT=3000 pnpm dev)", root, root),
                root
            ),
            ["app"]
        );
    }

    #[test]
    fn reads_package_manager_directory_options() {
        let dir = project(&[
            ("package.json", "{}"),
            ("app/package.json", APP_MANIFEST),
            ("site/package.json", APP_MANIFEST),
        ]);
        let root = dir.path();
        assert_eq!(
            relative(&entered_app_dirs("pnpm -C app dev", root, root), root),
            ["app"]
        );
        assert_eq!(
            relative(
                &entered_app_dirs("npm --prefix=site run dev", root, root),
                root
            ),
            ["site"]
        );
    }

    #[test]
    fn a_cwd_below_the_config_directory_counts() {
        let dir = project(&[("app/package.json", APP_MANIFEST)]);
        let root = dir.path();
        let dirs = entered_app_dirs("vite", &root.join("app"), root);
        assert_eq!(relative(&dirs, root), ["app"]);
    }

    #[test]
    fn the_config_directory_itself_is_not_an_app_directory() {
        let dir = project(&[("package.json", r#"{ "scripts": { "dev": "vite" } }"#)]);
        let root = dir.path();
        assert!(entered_app_dirs("pnpm exec vite dev", root, root).is_empty());
        assert!(entered_app_dirs("npm run dev", root, root).is_empty());
    }

    #[test]
    fn a_directory_without_package_json_is_not_an_app_directory() {
        let dir = project(&[("package.json", "{}"), ("server/main.ts", "")]);
        let root = dir.path();
        assert!(entered_app_dirs("cd server && node main.ts", root, root).is_empty());
    }

    #[test]
    fn a_directory_outside_the_config_directory_is_not_an_app_directory() {
        let dir = project(&[
            ("e2e/package.json", "{}"),
            ("web/package.json", APP_MANIFEST),
        ]);
        let root = dir.path();
        let config_dir = root.join("e2e");
        assert!(entered_app_dirs("cd ../web && npm run dev", &config_dir, &config_dir).is_empty());
    }

    #[test]
    fn an_interpolated_directory_stops_the_search() {
        let dir = project(&[("app/package.json", APP_MANIFEST)]);
        let root = dir.path();
        assert!(entered_app_dirs("cd $APP_DIR && cd app", root, root).is_empty());
    }
}
