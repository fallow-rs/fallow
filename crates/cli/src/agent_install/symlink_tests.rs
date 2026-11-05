//! A cloned repository can commit a symlink where fallow writes a harness
//! file. In project scope every step must leave the link target alone and
//! report a skipped row. In user scope a symlinked dotfile is normal.

use std::os::unix::fs::symlink;
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

fn command() -> mcp::McpCommand {
    mcp::McpCommand {
        command: "fallow-mcp".to_string(),
        args: Vec::new(),
        source: mcp::McpSource::Path,
    }
}

fn row<'a>(steps: &'a [StepReport], path: &str) -> &'a StepReport {
    steps
        .iter()
        .find(|s| s.path.as_deref() == Some(path))
        .unwrap_or_else(|| panic!("no row for {path} in {steps:?}"))
}

fn assert_symlink_skip(step: &StepReport) {
    assert_eq!(step.status, StepStatus::Skipped, "{step:?}");
    assert_eq!(
        step.reason.map(Reason::as_str),
        Some("symlinked_path"),
        "{step:?}"
    );
}

/// The row keeps its own `path`, and `detail` names the symlink.
fn assert_detail_names(step: &StepReport, link: &str) {
    let detail = step.detail.as_deref().unwrap_or_default();
    assert!(
        detail.starts_with(&format!("{link} is a symlink")),
        "{step:?}"
    );
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn forced_mcp_install_does_not_write_or_copy_a_symlinked_mcp_json() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.json");
    std::fs::write(&secret, "token = not json").unwrap();
    symlink(&secret, root.path().join(".mcp.json")).unwrap();

    let forced = Ctx {
        force: true,
        ..ctx(root.path(), Mode::Install)
    };
    let steps = mcp::install(&forced, &[Harness::Claude], Some(&command()));

    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "token = not json"
    );
    assert_eq!(entries(root.path()), vec![".mcp.json".to_string()]);
    assert_symlink_skip(row(&steps, ".mcp.json"));
}

#[test]
fn mcp_uninstall_does_not_edit_a_symlinked_mcp_json() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("mcp.json");
    let body = "{\n  \"mcpServers\": {\n    \"fallow\": { \"command\": \"fallow-mcp\", \"args\": [] },\n    \"other\": { \"command\": \"x\" }\n  }\n}\n";
    std::fs::write(&target, body).unwrap();
    symlink(&target, root.path().join(".mcp.json")).unwrap();

    let steps = mcp::uninstall(&ctx(root.path(), Mode::Uninstall), &[Harness::Claude]);

    assert_eq!(std::fs::read_to_string(&target).unwrap(), body);
    assert_symlink_skip(row(&steps, ".mcp.json"));
}

#[test]
fn mcp_install_does_not_follow_a_symlinked_parent_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join(".cursor")).unwrap();
    symlink(outside.path(), root.path().join(".codex")).unwrap();

    let steps = mcp::install(
        &ctx(root.path(), Mode::Install),
        &[Harness::Cursor, Harness::Codex],
        Some(&command()),
    );

    assert!(entries(outside.path()).is_empty());
    assert_symlink_skip(row(&steps, ".cursor/mcp.json"));
    assert_symlink_skip(row(&steps, ".codex/config.toml"));
}

#[test]
fn mcp_install_does_not_write_through_a_symlinked_backup_path() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim");
    std::fs::write(&victim, "keep").unwrap();
    std::fs::write(root.path().join(".mcp.json"), "not json").unwrap();
    symlink(&victim, root.path().join(".mcp.json.fallow-bak")).unwrap();

    let forced = Ctx {
        force: true,
        ..ctx(root.path(), Mode::Install)
    };
    let steps = mcp::install(&forced, &[Harness::Claude], Some(&command()));

    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(
        std::fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
        "not json"
    );
    assert_symlink_skip(row(&steps, ".mcp.json"));
    assert_detail_names(row(&steps, ".mcp.json"), ".mcp.json.fallow-bak");
}

#[test]
fn approval_does_not_follow_a_symlinked_claude_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join(".claude")).unwrap();

    let approve = Ctx {
        approve: true,
        ..ctx(root.path(), Mode::Install)
    };
    let steps = mcp::install(&approve, &[Harness::Claude], Some(&command()));

    assert!(entries(outside.path()).is_empty());
    assert_symlink_skip(row(&steps, ".claude/settings.local.json"));
}

#[test]
fn skill_install_and_uninstall_do_not_follow_a_symlinked_skills_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".agents")).unwrap();
    symlink(outside.path(), root.path().join(".agents/skills")).unwrap();

    let steps = skill::install(&ctx(root.path(), Mode::Install), &[Harness::Codex]);
    assert!(entries(outside.path()).is_empty());
    for step in &steps {
        assert_symlink_skip(step);
    }

    let fallow = outside.path().join("fallow");
    std::fs::create_dir_all(&fallow).unwrap();
    std::fs::write(fallow.join("SKILL.md"), "not fallow's").unwrap();
    let forced = Ctx {
        force: true,
        ..ctx(root.path(), Mode::Uninstall)
    };
    let steps = skill::uninstall(&forced, &[Harness::Codex]);
    assert!(fallow.join("SKILL.md").is_file());
    assert_symlink_skip(row(&steps, ".agents/skills/fallow"));
    assert_detail_names(row(&steps, ".agents/skills/fallow"), ".agents/skills");
}

#[test]
fn guide_does_not_write_or_delete_through_symlinked_agents_and_claude_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let agents = outside.path().join("agents.md");
    let claude = outside.path().join("claude.md");
    std::fs::write(&agents, "# theirs\n").unwrap();
    std::fs::write(&claude, "# theirs\n").unwrap();
    symlink(&agents, root.path().join("AGENTS.md")).unwrap();
    symlink(&claude, root.path().join("CLAUDE.md")).unwrap();

    let steps = guide::install(&ctx(root.path(), Mode::Install), &[Harness::Claude]);
    assert_symlink_skip(row(&steps, "AGENTS.md"));
    assert_symlink_skip(row(&steps, "CLAUDE.md"));
    assert_eq!(std::fs::read_to_string(&agents).unwrap(), "# theirs\n");
    assert_eq!(std::fs::read_to_string(&claude).unwrap(), "# theirs\n");

    let steps = guide::uninstall(&ctx(root.path(), Mode::Uninstall), &[Harness::Claude]);
    assert_symlink_skip(row(&steps, "AGENTS.md"));
    assert_symlink_skip(row(&steps, "CLAUDE.md"));
}

#[test]
fn hooks_do_not_write_through_symlinked_gate_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let settings = outside.path().join("settings.json");
    std::fs::write(&settings, "{}\n").unwrap();
    std::fs::create_dir_all(root.path().join(".claude")).unwrap();
    symlink(&settings, root.path().join(".claude/settings.json")).unwrap();
    symlink(outside.path(), root.path().join(".codex")).unwrap();

    let steps = hooks::install(
        &ctx(root.path(), Mode::Install),
        &[Harness::Claude, Harness::Codex],
    );

    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}\n");
    assert_eq!(entries(outside.path()), vec!["settings.json".to_string()]);
    assert!(!root.path().join(".claude/hooks").exists());
    assert!(!root.path().join("AGENTS.md").exists());
    let claude: Vec<&StepReport> = steps
        .iter()
        .filter(|s| s.harness == Some(Harness::Claude))
        .collect();
    let codex: Vec<&StepReport> = steps
        .iter()
        .filter(|s| s.harness == Some(Harness::Codex))
        .collect();
    assert_eq!(claude.len(), 1, "{steps:?}");
    assert_eq!(codex.len(), 1, "{steps:?}");
    assert_symlink_skip(claude[0]);
    assert_symlink_skip(codex[0]);
    assert_eq!(claude[0].path.as_deref(), Some(".claude/settings.json"));
    assert_eq!(codex[0].path.as_deref(), Some(".codex/hooks.json"));
    assert_detail_names(codex[0], ".codex");
}

#[test]
fn user_scope_writes_through_symlinked_dotfiles() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let dotfiles = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dotfiles.path().join("cursor")).unwrap();
    symlink(dotfiles.path().join("cursor"), home.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(dotfiles.path().join("codex")).unwrap();
    symlink(dotfiles.path().join("codex"), home.path().join(".codex")).unwrap();

    let user = Ctx {
        home: Some(home.path().to_path_buf()),
        user: true,
        ..ctx(root.path(), Mode::Install)
    };
    let mut steps = mcp::install(&user, &[Harness::Cursor], Some(&command()));
    steps.extend(hooks::install(&user, &[Harness::Codex]));

    assert!(
        steps.iter().all(|s| s.status == StepStatus::Written),
        "{steps:?}"
    );
    assert!(dotfiles.path().join("cursor/mcp.json").is_file());
    assert!(dotfiles.path().join("codex/hooks.json").is_file());
}

fn ship_skill(root: &Path) -> std::path::PathBuf {
    let shipped = root.join("node_modules/fallow/skills/fallow");
    std::fs::create_dir_all(shipped.join("agents")).unwrap();
    std::fs::write(
        shipped.join("SKILL.md"),
        "---\nname: fallow\ndescription: shipped\n---\n# Fallow\n",
    )
    .unwrap();
    shipped
}

#[test]
fn skill_install_does_not_copy_a_package_file_that_links_outside_the_package() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("id_ed25519");
    std::fs::write(&secret, "PRIVATE KEY").unwrap();
    let shipped = ship_skill(root.path());
    symlink(&secret, shipped.join("agents/openai.yaml")).unwrap();

    let steps = skill::install(&ctx(root.path(), Mode::Install), &[Harness::Codex]);

    let installed = root.path().join(".agents/skills/fallow");
    assert_eq!(
        row(&steps, ".agents/skills/fallow").status,
        StepStatus::Written
    );
    let copied = std::fs::read_to_string(installed.join("agents/openai.yaml")).unwrap_or_default();
    assert!(!copied.contains("PRIVATE KEY"), "{copied}");
    let skill_md = std::fs::read_to_string(installed.join("SKILL.md")).unwrap();
    assert!(skill_md.contains("skill=embedded"), "{skill_md}");
}

#[test]
fn skill_install_reads_a_package_that_a_link_places_elsewhere() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let shipped = ship_skill(store.path());
    std::fs::write(shipped.join("agents/openai.yaml"), "interface: {}\n").unwrap();
    std::fs::create_dir_all(root.path().join("node_modules")).unwrap();
    symlink(
        store.path().join("node_modules/fallow"),
        root.path().join("node_modules/fallow"),
    )
    .unwrap();

    let steps = skill::install(&ctx(root.path(), Mode::Install), &[Harness::Codex]);

    assert_eq!(
        row(&steps, ".agents/skills/fallow").status,
        StepStatus::Written
    );
    let installed = root.path().join(".agents/skills/fallow");
    let skill_md = std::fs::read_to_string(installed.join("SKILL.md")).unwrap();
    assert!(skill_md.contains("skill=stub"), "{skill_md}");
    assert_eq!(
        std::fs::read_to_string(installed.join("agents/openai.yaml")).unwrap(),
        "interface: {}\n"
    );
}

#[test]
fn guide_install_writes_through_a_link_that_stays_in_the_project() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("AGENTS.md"), "# Project\n").unwrap();
    symlink("AGENTS.md", root.path().join("CLAUDE.md")).unwrap();

    let steps = guide::install(&ctx(root.path(), Mode::Install), &[Harness::Claude]);

    for path in ["AGENTS.md", "CLAUDE.md"] {
        let step = row(&steps, path);
        assert_ne!(step.reason, Some(Reason::SymlinkedPath), "{step:?}");
        assert_ne!(step.status, StepStatus::Failed, "{step:?}");
    }
    assert!(
        std::fs::symlink_metadata(root.path().join("CLAUDE.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn skill_install_writes_through_a_skills_directory_link_in_the_project() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".agents/skills")).unwrap();
    std::fs::create_dir_all(root.path().join(".claude")).unwrap();
    symlink("../.agents/skills", root.path().join(".claude/skills")).unwrap();

    let steps = skill::install(&ctx(root.path(), Mode::Install), &[Harness::Claude]);

    assert_eq!(
        row(&steps, ".claude/skills/fallow").status,
        StepStatus::Written
    );
    assert!(root.path().join(".agents/skills/fallow/SKILL.md").is_file());
}

#[test]
fn guide_install_refuses_a_dangling_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let planted = outside.path().join("later.md");
    symlink(&planted, root.path().join("AGENTS.md")).unwrap();

    let steps = guide::install(&ctx(root.path(), Mode::Install), &[]);

    assert!(!planted.exists());
    assert_symlink_skip(row(&steps, "AGENTS.md"));
    assert_detail_names(row(&steps, "AGENTS.md"), "AGENTS.md");
}
