//! Claude Code plugin-install hint.
//!
//! Claude Code reads a `<claude-code-hint />` line from the stderr of a
//! command that it runs. It removes the line from the output that the model
//! sees and asks the user in the terminal to install the named plugin. The
//! format, the stream and the session markers follow the Claude Code plugin
//! hints reference.
//!
//! Claude Code shows the prompt only for plugins in an official marketplace.
//! The hint names `fallow@claude-plugins-official`, so it has no effect until
//! that marketplace lists the plugin.
//!
//! The hint is one more unsolicited stderr notice in the run epilogue. It is
//! never written to stdout, never written for a machine-readable format and
//! never written under `--quiet`: Claude Code reads the hint from stderr, and
//! `--quiet` is the flag that keeps stderr free of notices.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fallow_config::OutputFormat;

/// Opt-out environment variable for the hint.
pub const HINT_ENV: &str = "FALLOW_CLAUDE_CODE_HINT";

/// Plugin identifier in the hint, in `name@marketplace` form.
const PLUGIN_ID: &str = "fallow@claude-plugins-official";

/// Plugin name that the installed-plugin check matches in any marketplace.
const PLUGIN_NAME: &str = "fallow";

/// Session markers that Claude Code sets in the processes that it starts.
/// `CLAUDECODE` exists in all versions. `CLAUDE_CODE_CHILD_SESSION` exists
/// from version 2.1.172.
const SESSION_ENV: [&str; 2] = ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION"];

/// Environment variable that moves the Claude Code config directory.
const CLAUDE_CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

/// Set after the first emit, so one process writes the hint at most once.
static EMITTED: AtomicBool = AtomicBool::new(false);

/// Facts about the run that decide if the hint can show.
#[derive(Clone, Copy, Debug)]
struct Gate {
    claude_session: bool,
    human: bool,
    quiet: bool,
    ci: bool,
    opted_out: bool,
    suggestions_off: bool,
    child_run: bool,
    run_completed: bool,
}

/// Write the hint to stderr when the run qualifies. Returns `true` when the
/// hint was written, so the caller can skip the next notice in the chain.
///
/// `run_completed` is `true` for exit code 0 and for exit code 1 (findings).
/// `child_run` is `true` for a process that another fallow process started.
pub fn maybe_emit(
    output: OutputFormat,
    quiet: bool,
    root: Option<&Path>,
    child_run: bool,
    run_completed: bool,
) -> bool {
    let Some(root) = root else {
        return false;
    };
    let gate = Gate {
        claude_session: SESSION_ENV.iter().any(|key| env_non_empty(key)),
        human: matches!(output, OutputFormat::Human),
        quiet,
        ci: fallow_engine::ci_env::is_ci(),
        opted_out: is_off(std::env::var(HINT_ENV).ok().as_deref()),
        suggestions_off: !crate::report::suggestions::suggestions_enabled(),
        child_run,
        run_completed,
    };
    if !should_consider(gate) {
        return false;
    }
    if plugin_known(root, claude_config_dir().as_deref()) {
        return false;
    }
    if EMITTED.swap(true, Ordering::Relaxed) {
        return false;
    }
    eprintln!("{}", hint_line());
    true
}

/// Pure gate over the environment facts. The file reads happen after it.
fn should_consider(gate: Gate) -> bool {
    gate.claude_session
        && gate.human
        && !gate.quiet
        && !gate.ci
        && !gate.opted_out
        && !gate.suggestions_off
        && !gate.child_run
        && gate.run_completed
}

fn hint_line() -> String {
    format!(r#"<claude-code-hint v="1" type="plugin" value="{PLUGIN_ID}" />"#)
}

fn env_non_empty(key: &str) -> bool {
    std::env::var_os(key).is_some_and(|value| !value.is_empty())
}

/// `true` for an explicit off value of the opt-out variable.
fn is_off(value: Option<&str>) -> bool {
    value.is_some_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        )
    })
}

/// The Claude Code config directory: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn claude_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(CLAUDE_CONFIG_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    // Claude Code on Windows resolves the home directory from `USERPROFILE`
    // when `HOME` is not set.
    crate::setup_hooks::home_dir()
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
        })
        .map(|home| home.join(".claude"))
}

/// The project `.claude` directories that can hold a Claude Code signal: the
/// analysis root and each ancestor up to the repository root (the first
/// directory with a `.git` entry). A Claude Code session that starts at the
/// repository root keeps its settings there, also for `--root packages/app`.
fn project_claude_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for dir in root.ancestors() {
        dirs.push(dir.join(".claude"));
        if dir.join(".git").exists() {
            break;
        }
    }
    dirs
}

/// `true` when the project or the user already has Fallow set up for Claude
/// Code. The check reads a few small files and never fails: a file that is
/// missing or not valid JSON counts as no signal.
///
/// Signals, cheapest first:
/// - `skills/fallow/SKILL.md` in a project `.claude` directory (the root or
///   an ancestor up to the repository root) or in the
///   user Claude Code directory (`fallow agent install`, with or without
///   `--user`).
/// - An `enabledPlugins` key for the `fallow` plugin in the project
///   `settings.json` or `settings.local.json`, or in the user `settings.json`.
///   A key with the value `false` also counts: the user knows the plugin.
/// - An entry for the `fallow` plugin in `plugins/installed_plugins.json`
///   with user scope, or with a `projectPath` equal to the project root.
fn plugin_known(root: &Path, claude_dir: Option<&Path>) -> bool {
    let project_dirs = project_claude_dirs(root);
    let skill_dirs = project_dirs.iter().map(PathBuf::as_path).chain(claude_dir);
    if skill_dirs
        .map(|dir| dir.join("skills/fallow/SKILL.md"))
        .any(|skill| skill.is_file())
    {
        return true;
    }
    let mut settings: Vec<PathBuf> = project_dirs
        .iter()
        .flat_map(|dir| [dir.join("settings.json"), dir.join("settings.local.json")])
        .collect();
    if let Some(dir) = claude_dir {
        settings.push(dir.join("settings.json"));
    }
    if settings.iter().any(|path| settings_enable_plugin(path)) {
        return true;
    }
    claude_dir.is_some_and(|dir| installed_for(&dir.join("plugins/installed_plugins.json"), root))
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn is_fallow_plugin_key(key: &str) -> bool {
    key.split_once('@')
        .map_or(key, |(name, _marketplace)| name)
        .eq(PLUGIN_NAME)
}

fn settings_enable_plugin(path: &Path) -> bool {
    read_json(path)
        .as_ref()
        .and_then(|value| value.get("enabledPlugins"))
        .and_then(serde_json::Value::as_object)
        .is_some_and(|plugins| plugins.keys().any(|key| is_fallow_plugin_key(key)))
}

fn installed_for(path: &Path, root: &Path) -> bool {
    let Some(value) = read_json(path) else {
        return false;
    };
    let Some(plugins) = value.get("plugins").and_then(serde_json::Value::as_object) else {
        return false;
    };
    let root = canonical(root);
    plugins
        .iter()
        .filter(|(key, _)| is_fallow_plugin_key(key))
        .filter_map(|(_, entries)| entries.as_array())
        .flatten()
        .any(|entry| {
            let scope = entry.get("scope").and_then(serde_json::Value::as_str);
            let project = entry.get("projectPath").and_then(serde_json::Value::as_str);
            scope == Some("user") || project.is_some_and(|path| canonical(Path::new(path)) == root)
        })
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: Gate = Gate {
        claude_session: true,
        human: true,
        quiet: false,
        ci: false,
        opted_out: false,
        suggestions_off: false,
        child_run: false,
        run_completed: true,
    };

    #[test]
    fn gate_opens_only_when_every_condition_holds() {
        assert!(should_consider(OPEN));
        let closed = [
            Gate {
                claude_session: false,
                ..OPEN
            },
            Gate {
                human: false,
                ..OPEN
            },
            Gate {
                quiet: true,
                ..OPEN
            },
            Gate { ci: true, ..OPEN },
            Gate {
                opted_out: true,
                ..OPEN
            },
            Gate {
                suggestions_off: true,
                ..OPEN
            },
            Gate {
                child_run: true,
                ..OPEN
            },
            Gate {
                run_completed: false,
                ..OPEN
            },
        ];
        for gate in closed {
            assert!(!should_consider(gate), "{gate:?} must stay closed");
        }
    }

    #[test]
    fn hint_line_uses_the_documented_syntax() {
        assert_eq!(
            hint_line(),
            r#"<claude-code-hint v="1" type="plugin" value="fallow@claude-plugins-official" />"#
        );
    }

    #[test]
    fn opt_out_parses_off_values_only() {
        for off in ["off", "0", "false", "no", "disabled", " OFF "] {
            assert!(is_off(Some(off)), "{off} must opt out");
        }
        for on in ["on", "1", "true", "", "yes"] {
            assert!(!is_off(Some(on)), "{on} must not opt out");
        }
        assert!(!is_off(None));
    }

    #[test]
    fn plugin_key_matches_the_fallow_name_in_any_marketplace() {
        assert!(is_fallow_plugin_key("fallow@fallow-skills"));
        assert!(is_fallow_plugin_key("fallow@claude-plugins-official"));
        assert!(is_fallow_plugin_key("fallow"));
        assert!(!is_fallow_plugin_key("fallow-review@fallow-skills"));
        assert!(!is_fallow_plugin_key("other@fallow"));
    }

    #[test]
    fn empty_project_and_config_dir_give_no_signal() {
        let root = tempfile::tempdir().expect("root");
        let claude = tempfile::tempdir().expect("claude dir");
        assert!(!plugin_known(root.path(), Some(claude.path())));
        assert!(!plugin_known(root.path(), None));
    }

    #[test]
    fn project_skill_counts_as_set_up() {
        let root = tempfile::tempdir().expect("root");
        let skill = root.path().join(".claude/skills/fallow");
        std::fs::create_dir_all(&skill).expect("skill dir");
        std::fs::write(skill.join("SKILL.md"), "---\nname: fallow\n---\n").expect("skill");
        assert!(plugin_known(root.path(), None));
    }

    #[test]
    fn user_scope_skill_counts_as_set_up() {
        let root = tempfile::tempdir().expect("root");
        let claude = tempfile::tempdir().expect("claude dir");
        let skill = claude.path().join("skills/fallow");
        std::fs::create_dir_all(&skill).expect("skill dir");
        std::fs::write(skill.join("SKILL.md"), "---\nname: fallow\n---\n").expect("skill");
        assert!(plugin_known(root.path(), Some(claude.path())));
    }

    #[test]
    fn enabled_plugins_in_local_or_user_settings_count() {
        let root = tempfile::tempdir().expect("root");
        let claude = tempfile::tempdir().expect("claude dir");
        std::fs::write(
            claude.path().join("settings.json"),
            r#"{"enabledPlugins":{"fallow@fallow-skills":false}}"#,
        )
        .expect("user settings");
        assert!(plugin_known(root.path(), Some(claude.path())));

        let root = tempfile::tempdir().expect("root");
        let project_claude = root.path().join(".claude");
        std::fs::create_dir_all(&project_claude).expect("project .claude");
        std::fs::write(
            project_claude.join("settings.local.json"),
            r#"{"enabledPlugins":{"fallow@claude-plugins-official":true}}"#,
        )
        .expect("local settings");
        assert!(plugin_known(root.path(), None));
    }

    #[test]
    fn other_plugins_and_invalid_json_give_no_signal() {
        let root = tempfile::tempdir().expect("root");
        let claude = tempfile::tempdir().expect("claude dir");
        std::fs::write(
            claude.path().join("settings.json"),
            r#"{"enabledPlugins":{"fallow-review@x":true,"other@fallow":true}}"#,
        )
        .expect("user settings");
        let plugins = claude.path().join("plugins");
        std::fs::create_dir_all(&plugins).expect("plugins dir");
        std::fs::write(plugins.join("installed_plugins.json"), "{not json").expect("write");
        assert!(!plugin_known(root.path(), Some(claude.path())));
    }

    #[test]
    fn project_scoped_install_counts_only_for_its_own_project() {
        let root = tempfile::tempdir().expect("root");
        let other = tempfile::tempdir().expect("other project");
        let claude = tempfile::tempdir().expect("claude dir");
        let plugins = claude.path().join("plugins");
        std::fs::create_dir_all(&plugins).expect("plugins dir");
        let write = |project: &Path| {
            let body = serde_json::json!({
                "version": 2,
                "plugins": {
                    "fallow@fallow-skills": [
                        { "scope": "project", "projectPath": project.to_string_lossy() }
                    ]
                }
            });
            std::fs::write(plugins.join("installed_plugins.json"), body.to_string())
                .expect("installed_plugins");
        };
        write(other.path());
        assert!(!plugin_known(root.path(), Some(claude.path())));
        write(root.path());
        assert!(plugin_known(root.path(), Some(claude.path())));
    }
}
