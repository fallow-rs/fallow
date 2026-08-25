use std::path::Path;

use super::*;

fn ctx(root: &Path, mode: Mode) -> Ctx {
    Ctx {
        root: root.to_path_buf(),
        home: None,
        user: false,
        dry_run: false,
        force: false,
        approve: false,
        gitignore_claude: false,
        mode,
    }
}

#[test]
fn nothing_detected_selects_no_harness() {
    let dir = tempfile::tempdir().unwrap();
    let found = hosts::detect_with(dir.path(), None, |_| false);
    assert!(found.is_empty());
}

#[test]
fn detection_reads_project_home_and_session_signals() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    let found = hosts::detect_with(dir.path(), Some(home.path()), |key| key == "CLAUDECODE");
    let names: Vec<Harness> = found.iter().map(|d| d.harness).collect();
    assert_eq!(
        names,
        vec![Harness::Claude, Harness::Codex, Harness::Cursor]
    );
    assert_eq!(found[0].evidence, vec!["$CLAUDECODE".to_string()]);
    assert_eq!(found[1].evidence, vec!["~/.codex".to_string()]);
    assert_eq!(found[2].evidence, vec![".cursor".to_string()]);
}

#[test]
fn guide_scaffolds_agents_md_and_claude_import_then_stays_stable() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx(dir.path(), Mode::Install);
    let steps = guide::install(&ctx, &[Harness::Claude]);
    assert!(
        steps.iter().all(|s| s.status == StepStatus::Written),
        "{steps:?}"
    );
    let agents = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("<!-- fallow:agent-install v1 authored sha256="));
    assert!(agents.contains("## Fallow task map"));
    let claude = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains("@AGENTS.md"));

    let again = guide::install(&ctx, &[Harness::Claude]);
    assert!(
        again.iter().all(|s| s.status == StepStatus::Unchanged),
        "{again:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        agents
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        claude
    );
}

#[test]
fn guide_uninstall_deletes_authored_files_but_keeps_edited_ones() {
    let dir = tempfile::tempdir().unwrap();
    let install = ctx(dir.path(), Mode::Install);
    guide::install(&install, &[Harness::Claude]);
    let agents_path = dir.path().join("AGENTS.md");
    let mut edited = std::fs::read_to_string(&agents_path).unwrap();
    edited.push_str("\n## Team notes\n\nKeep me.\n");
    std::fs::write(&agents_path, edited).unwrap();

    let uninstall = ctx(dir.path(), Mode::Uninstall);
    let steps = guide::uninstall(&uninstall, &[Harness::Claude]);
    assert!(!dir.path().join("CLAUDE.md").exists(), "{steps:?}");
    let kept = std::fs::read_to_string(&agents_path).unwrap();
    assert!(kept.contains("Keep me."));
    assert!(!kept.contains("## Fallow task map"));
}

#[test]
fn guide_uninstall_strips_import_from_an_edited_authored_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let install = ctx(dir.path(), Mode::Install);
    guide::install(&install, &[Harness::Claude]);
    let path = dir.path().join("CLAUDE.md");
    let mut edited = std::fs::read_to_string(&path).unwrap();
    edited.push_str("\n## Mine\n\nKeep me.\n");
    std::fs::write(&path, edited).unwrap();

    let uninstall = ctx(dir.path(), Mode::Uninstall);
    let steps = guide::uninstall(&uninstall, &[Harness::Claude]);
    let claude = steps
        .iter()
        .find(|s| s.path.as_deref() == Some("CLAUDE.md"))
        .unwrap();
    assert_eq!(claude.status, StepStatus::Removed, "{steps:?}");
    let kept = std::fs::read_to_string(&path).unwrap();
    assert!(kept.contains("Keep me."));
    assert!(!kept.contains("@AGENTS.md"), "{kept}");
    assert!(!kept.contains("claude-import"), "{kept}");
}

#[test]
fn guide_appends_import_block_to_existing_claude_md_and_removes_it_again() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "# Mine\n\nRules.\n").unwrap();
    let install = ctx(dir.path(), Mode::Install);
    guide::install(&install, &[Harness::Claude]);
    let text = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(text.starts_with("# Mine\n"));
    assert!(text.contains("claude-import:start"));
    assert!(text.contains("@AGENTS.md"));

    let uninstall = ctx(dir.path(), Mode::Uninstall);
    guide::uninstall(&uninstall, &[Harness::Claude]);
    let text = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert_eq!(text, "# Mine\n\nRules.\n");
}

#[test]
fn skill_refuses_foreign_skill_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join(".agents").join("skills").join("fallow");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: fallow\n---\nmaintainer skill\n",
    )
    .unwrap();
    let ctx = ctx(dir.path(), Mode::Install);
    let steps = skill::install(&ctx, &[Harness::Codex]);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].status, StepStatus::Refused, "{steps:?}");
    assert_eq!(steps[0].reason, Some("skill_name_taken"));
    assert_eq!(
        std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap(),
        "---\nname: fallow\n---\nmaintainer skill\n"
    );
}

#[test]
fn skill_writes_stub_when_node_modules_ships_the_skill() {
    let dir = tempfile::tempdir().unwrap();
    let shipped = dir.path().join("node_modules/fallow/skills/fallow");
    std::fs::create_dir_all(shipped.join("agents")).unwrap();
    std::fs::write(
        shipped.join("SKILL.md"),
        "---\nname: fallow\ndescription: Shipped description.\nlicense: MIT\n---\n\n# Full skill\n",
    )
    .unwrap();
    std::fs::write(
        shipped.join("agents/openai.yaml"),
        "interface:\n  display_name: \"Fallow\"\n",
    )
    .unwrap();
    let ctx = ctx(dir.path(), Mode::Install);
    let steps = skill::install(&ctx, &[Harness::Claude, Harness::Cursor]);
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert!(
        steps.iter().all(|s| s.status == StepStatus::Written),
        "{steps:?}"
    );
    for base in [".agents", ".claude"] {
        let text =
            std::fs::read_to_string(dir.path().join(base).join("skills/fallow/SKILL.md")).unwrap();
        assert!(text.starts_with("---\nname: fallow\ndescription: Shipped description.\n"));
        assert!(text.contains("skill=stub version="));
        assert!(text.contains("node_modules/fallow/skills/fallow/SKILL.md"));
        assert!(text.contains("## Fallow task map"));
        assert!(
            dir.path()
                .join(base)
                .join("skills/fallow/agents/openai.yaml")
                .is_file()
        );
    }

    let again = skill::install(&ctx, &[Harness::Claude, Harness::Cursor]);
    assert!(
        again.iter().all(|s| s.status == StepStatus::Unchanged),
        "{again:?}"
    );

    let uninstall = Ctx {
        mode: Mode::Uninstall,
        ..ctx
    };
    let removed = skill::uninstall(&uninstall, &[Harness::Claude, Harness::Cursor]);
    assert!(
        removed.iter().all(|s| s.status == StepStatus::Removed),
        "{removed:?}"
    );
    assert!(!dir.path().join(".claude/skills").exists());
    assert!(!dir.path().join(".agents/skills").exists());
}

#[test]
fn skill_embedded_copy_round_trips_when_available() {
    if skill::EMBEDDED_SKILL.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx(dir.path(), Mode::Install);
    let steps = skill::install(&ctx, &[Harness::Codex]);
    assert_eq!(steps[0].status, StepStatus::Written, "{steps:?}");
    let skill_dir = dir.path().join(".agents/skills/fallow");
    let text = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(text.starts_with("---\nname: fallow\n"));
    assert!(text.contains("skill=embedded version="));
    assert!(skill_dir.join("references/cli-reference.md").is_file());
    assert_eq!(
        skill::inspect(&skill_dir),
        skill::SkillState::Managed {
            flavor: skill::Flavor::Embedded,
            version: env!("CARGO_PKG_VERSION").to_string()
        }
    );
}

#[test]
fn mcp_command_prefers_node_modules_then_path_then_self() {
    let node = mcp::resolve_command_with(true, || None, || None).unwrap();
    assert_eq!(node.command, "npx");
    assert_eq!(node.args, vec!["--no", "fallow-mcp"]);
    let path =
        mcp::resolve_command_with(false, || Some("/usr/local/bin/fallow-mcp".into()), || None)
            .unwrap();
    assert_eq!(path.command, "fallow-mcp");
    assert!(path.args.is_empty());
    let own = mcp::resolve_command_with(false, || None, || Some("/opt/fallow".into())).unwrap();
    assert_eq!(own.args, vec!["mcp-server"]);
    assert!(mcp::resolve_command_with(false, || None, || None).is_none());
}

#[test]
fn mcp_json_merge_preserves_other_servers_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".mcp.json");
    std::fs::write(
        &path,
        "{\n  \"mcpServers\": {\n    \"other\": { \"command\": \"x\" }\n  },\n  \"custom\": 1\n}\n",
    )
    .unwrap();
    let entry =
        serde_json::json!({"type": "stdio", "command": "npx", "args": ["--no", "fallow-mcp"]});
    assert_eq!(
        mcp::merge_json_server(&path, "mcpServers", Some(&entry), false, false).unwrap(),
        mcp::FileOutcome::Changed
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["custom"], 1);
    assert_eq!(value["mcpServers"]["other"]["command"], "x");
    assert_eq!(value["mcpServers"]["fallow"]["args"][1], "fallow-mcp");
    assert_eq!(
        mcp::merge_json_server(&path, "mcpServers", Some(&entry), false, false).unwrap(),
        mcp::FileOutcome::Unchanged
    );
    assert_eq!(
        mcp::merge_json_server(&path, "mcpServers", None, false, false).unwrap(),
        mcp::FileOutcome::Changed
    );
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(value["mcpServers"].get("fallow").is_none());
    assert_eq!(value["mcpServers"]["other"]["command"], "x");
}

#[test]
fn mcp_json_merge_refuses_invalid_json_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".mcp.json");
    std::fs::write(&path, "{ not json").unwrap();
    let entry = serde_json::json!({"command": "fallow-mcp"});
    assert_eq!(
        mcp::merge_json_server(&path, "mcpServers", Some(&entry), false, false).unwrap(),
        mcp::FileOutcome::InvalidPreserved
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    assert_eq!(
        mcp::merge_json_server(&path, "mcpServers", Some(&entry), false, true).unwrap(),
        mcp::FileOutcome::Changed
    );
}

#[test]
fn codex_toml_merge_keeps_comments_and_other_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "# my codex config\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
    )
    .unwrap();
    let command = mcp::McpCommand {
        command: "npx".to_string(),
        args: vec!["--no".to_string(), "fallow-mcp".to_string()],
        source: mcp::McpSource::NodeModules,
    };
    assert_eq!(
        mcp::merge_codex(&path, Some(&command), false, false).unwrap(),
        mcp::FileOutcome::Changed
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# my codex config\nmodel = \"gpt-5\"\n"));
    assert!(text.contains("[mcp_servers.other]\ncommand = \"x\"\n"));
    assert!(
        text.contains(
            "[mcp_servers.fallow]\ncommand = \"npx\"\nargs = [\"--no\", \"fallow-mcp\"]\n"
        )
    );
    assert!(!text.contains("\n[mcp_servers]\n"));
    assert_eq!(
        mcp::merge_codex(&path, Some(&command), false, false).unwrap(),
        mcp::FileOutcome::Unchanged
    );
    assert_eq!(
        mcp::merge_codex(&path, None, false, false).unwrap(),
        mcp::FileOutcome::Changed
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("fallow"));
    assert!(text.contains("[mcp_servers.other]"));
}

#[test]
fn dry_run_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
    let ctx = Ctx {
        dry_run: true,
        ..ctx(dir.path(), Mode::Install)
    };
    let mut steps = guide::install(&ctx, &[Harness::Claude]);
    steps.extend(skill::install(&ctx, &[Harness::Claude]));
    let command = mcp::McpCommand {
        command: "fallow-mcp".to_string(),
        args: Vec::new(),
        source: mcp::McpSource::Path,
    };
    steps.extend(mcp::install(&ctx, &[Harness::Claude], Some(&command)));
    steps.extend(hooks::install(&ctx, &[Harness::Claude]));
    assert!(
        steps.iter().any(|s| s.status == StepStatus::Written),
        "{steps:?}"
    );
    let mut entries: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, vec![".claude".to_string()]);
    assert!(
        std::fs::read_dir(dir.path().join(".claude"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn approval_is_opt_in_and_lands_in_local_settings() {
    let dir = tempfile::tempdir().unwrap();
    let command = mcp::McpCommand {
        command: "fallow-mcp".to_string(),
        args: Vec::new(),
        source: mcp::McpSource::Path,
    };
    let without = ctx(dir.path(), Mode::Install);
    let steps = mcp::install(&without, &[Harness::Claude], Some(&command));
    let approval = steps
        .iter()
        .find(|s| s.path.as_deref() == Some(".claude/settings.local.json"))
        .unwrap();
    assert_eq!(approval.status, StepStatus::Skipped);
    assert_eq!(approval.reason, Some("approval_not_requested"));
    assert!(!dir.path().join(".claude/settings.local.json").exists());

    let with = Ctx {
        approve: true,
        ..without
    };
    let steps = mcp::install(&with, &[Harness::Claude], Some(&command));
    let approval = steps
        .iter()
        .find(|s| s.path.as_deref() == Some(".claude/settings.local.json"))
        .unwrap();
    assert_eq!(approval.status, StepStatus::Written, "{steps:?}");
    assert_eq!(approval.scope, Scope::Local);
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(value["enabledMcpjsonServers"][0], "fallow");
}

#[test]
fn cursor_hooks_are_reported_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx(dir.path(), Mode::Install);
    let steps = hooks::install(&ctx, &[Harness::Cursor]);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].status, StepStatus::Skipped);
    assert_eq!(steps[0].reason, Some("unsupported_harness"));
}

#[test]
fn display_path_uses_root_then_home_then_absolute() {
    let root = Path::new("/work/app");
    let home = Path::new("/home/me");
    assert_eq!(
        display_path(root, Some(home), Path::new("/work/app/.mcp.json")),
        ".mcp.json"
    );
    assert_eq!(
        display_path(root, Some(home), Path::new("/home/me/.codex/config.toml")),
        "~/.codex/config.toml"
    );
    assert_eq!(
        display_path(root, None, Path::new("/elsewhere/x")),
        "/elsewhere/x"
    );
}

#[test]
fn human_report_groups_scopes_and_summarizes_refusals() {
    let report = Report {
        root: "/work/app".to_string(),
        mode: "install",
        dry_run: true,
        harnesses: vec![Harness::Claude],
        detected: true,
        evidence: Vec::new(),
        steps: vec![
            StepReport::new(None, Step::Guide, StepStatus::Written, Scope::Shared)
                .detail("scaffold with the fallow task map")
                .with_path("AGENTS.md"),
            StepReport::new(
                Some(Harness::Claude),
                Step::Mcp,
                StepStatus::Skipped,
                Scope::Local,
            )
            .reason("approval_not_requested")
            .detail("pass --approve to pre-approve the project MCP server for yourself")
            .with_path(".claude/settings.local.json"),
            StepReport::new(
                Some(Harness::Claude),
                Step::Skill,
                StepStatus::Refused,
                Scope::Shared,
            )
            .reason("skill_name_taken")
            .with_path(".claude/skills/fallow"),
        ],
        next_actions: vec![NextAction {
            id: "recommend-config",
            command: "fallow recommend --format json".to_string(),
            reason: "No fallow config was found.".to_string(),
            mutating: false,
        }],
    };
    let text = render_human(&report);
    let expected = "\
fallow agent install (dry run)
  root: /work/app
  harnesses: claude (detected)

Shared with your team (commit these):
  AGENTS.md                                 would write    guide             scaffold with the fallow task map
  .claude/skills/fallow                     refused        skill (claude)    skill_name_taken

Local to you:
  .claude/settings.local.json               skipped        mcp (claude)      approval_not_requested: pass --approve to pre-approve the project MCP server for yourself

1 step refused (existing content is not fallow-managed; pass --force to replace it); every other step still ran. Exit code 2.

Next:
  fallow recommend --format json
    No fallow config was found.
";
    assert_eq!(text, expected);
}

impl StepReport {
    fn with_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }
}

#[test]
fn resolve_root_prefers_git_toplevel_unless_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    let nested = root.join("packages").join("web");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(resolve_root(&nested, false), root);
    assert_eq!(resolve_root(&nested, true), nested);
}
