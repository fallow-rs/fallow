//! Config reads: parse errors without file content, regular files only, and
//! discovered config links that stay inside the project.

use super::*;

const MARKER: &str = "marker-7531";

/// A project directory below a temp dir, with a file next to the project.
fn project_with_outside_file(outside_name: &str, outside_content: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("project")).unwrap();
    std::fs::create_dir(dir.path().join("project/.git")).unwrap();
    std::fs::write(dir.path().join(outside_name), outside_content).unwrap();
    dir
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn toml_parse_error_of_an_extended_file_names_the_position_only() {
    let dir = project_with_outside_file("outside.txt", &format!("a = 1\nkey={MARKER}\n"));
    let config = dir.path().join("project/.fallowrc.json");
    std::fs::write(&config, r#"{"extends": ["../outside.txt"]}"#).unwrap();

    let message = FallowConfig::load(&config).unwrap_err().to_string();
    assert!(!message.contains(MARKER), "{message}");
    assert!(message.contains("outside.txt"), "{message}");
    assert!(message.contains("line 2"), "{message}");
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn type_error_of_an_extended_file_omits_the_value() {
    let dir = project_with_outside_file("outside.json", &format!(r#"{{"entry": "{MARKER}"}}"#));
    let config = dir.path().join("project/.fallowrc.json");
    std::fs::write(&config, r#"{"extends": ["../outside.json"]}"#).unwrap();

    let message = FallowConfig::load(&config).unwrap_err().to_string();
    assert!(!message.contains(MARKER), "{message}");
    assert!(message.contains("outside.json"), "{message}");
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn an_extended_file_outside_the_project_shows_no_field_name_or_own_message() {
    for content in [
        format!(r#"{{"{MARKER}": true}}"#),
        format!(r#"{{"keepExportTags": ["{MARKER}"]}}"#),
        format!("{{\n  \"{MARKER}\": ,\n}}"),
    ] {
        let dir = project_with_outside_file("outside.json", &content);
        let config = dir.path().join("project/.fallowrc.json");
        std::fs::write(&config, r#"{"extends": ["../outside.json"]}"#).unwrap();

        let message = FallowConfig::load(&config).unwrap_err().to_string();
        assert!(!message.contains(MARKER), "{message}");
        assert!(message.contains("outside.json"), "{message}");
    }
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn a_file_inside_the_project_keeps_the_full_error_text() {
    let dir = project_with_outside_file("outside.json", r#"{"entry": ["src/a.ts"]}"#);
    let project = dir.path().join("project");
    std::fs::write(project.join("base.json"), r#"{"unknownInside": true}"#).unwrap();
    let config = project.join(".fallowrc.json");
    std::fs::write(
        &config,
        r#"{"extends": ["../outside.json", "./base.json"]}"#,
    )
    .unwrap();

    let message = FallowConfig::load(&config).unwrap_err().to_string();
    assert!(message.contains("unknownInside"), "{message}");
    assert!(message.contains("base.json"), "{message}");
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn toml_syntax_error_of_the_project_config_omits_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("fallow.toml");
    std::fs::write(&config, format!("entry = [\"a\"]\nkey={MARKER}\n")).unwrap();

    let message = FallowConfig::load(&config).unwrap_err().to_string();
    assert!(!message.contains(MARKER), "{message}");
    assert!(message.contains("line 2"), "{message}");
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "file system access is blocked by Miri isolation")]
fn extends_of_a_device_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".fallowrc.json");
    let ups = "../".repeat(dir.path().components().count() + 1);
    std::fs::write(&config, format!(r#"{{"extends": ["{ups}dev/null"]}}"#)).unwrap();

    let message = FallowConfig::load(&config).unwrap_err().to_string();
    assert!(message.contains("not a regular file"), "{message}");
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn discovery_refuses_a_config_link_that_leaves_the_project() {
    let dir = project_with_outside_file("outside.json", r#"{"entry": ["src/a.ts"]}"#);
    let project = dir.path().join("project");
    std::os::unix::fs::symlink(
        dir.path().join("outside.json"),
        project.join(".fallowrc.json"),
    )
    .unwrap();

    let message = FallowConfig::find_and_load(&project).unwrap_err();
    assert!(message.contains(".fallowrc.json"), "{message}");
    assert!(!message.contains("outside.json"), "{message}");
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn discovery_gives_one_message_for_dangling_and_existing_outside_links() {
    let dir = project_with_outside_file("outside.json", "{}");
    let project = dir.path().join("project");
    let link = project.join(".fallowrc.json");
    std::os::unix::fs::symlink(dir.path().join("outside.json"), &link).unwrap();
    let existing = FallowConfig::find_and_load(&project).unwrap_err();

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(dir.path().join("missing.json"), &link).unwrap();
    let dangling = FallowConfig::find_and_load(&project).unwrap_err();
    assert_eq!(existing, dangling);
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn discovery_loads_a_config_link_inside_the_repository() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    std::fs::create_dir(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("packages/app")).unwrap();
    std::fs::write(repo.join("shared.json"), r#"{"entry": ["src/a.ts"]}"#).unwrap();
    std::os::unix::fs::symlink(
        "../../shared.json",
        repo.join("packages/app/.fallowrc.json"),
    )
    .unwrap();

    let (config, _) = FallowConfig::find_and_load(&repo.join("packages/app"))
        .unwrap()
        .unwrap();
    assert_eq!(config.entry, vec!["src/a.ts".to_owned()]);
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn shadowed_names_count_links_without_their_targets() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/nonexistent/fallow/x", dir.path().join(".fallowrc.jsonc"))
        .unwrap();
    assert_eq!(
        shadowed_config_names(dir.path(), 0),
        vec![".fallowrc.jsonc"]
    );
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn config_path_keys_must_stay_inside_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".fallowrc.json");
    for (body, field) in [
        (r#"{"codeowners": "/abs/CODEOWNERS"}"#, "codeowners"),
        (
            r#"{"health": {"coverage": "/tmp/coverage.json"}}"#,
            "health.coverage",
        ),
        (
            r#"{"audit": {"healthBaseline": "/a.json"}}"#,
            "audit.healthBaseline",
        ),
    ] {
        std::fs::write(&config, body).unwrap();
        let message = FallowConfig::load(&config).unwrap_err().to_string();
        assert!(message.contains(field), "{body}: {message}");
    }
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn relative_config_path_keys_load() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".fallowrc.json");
    std::fs::write(
        &config,
        r#"{"codeowners": ".github/CODEOWNERS",
            "health": {"coverage": "coverage/coverage-final.json"},
            "audit": {"deadCodeBaseline": "baselines/dead.json"}}"#,
    )
    .unwrap();
    FallowConfig::load(&config).unwrap();
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn zone_paths_are_checked_after_normalization() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".fallowrc.json");
    for (body, field) in [
        (
            r#"{"boundaries": {"zones": [{"name": "a", "root": ".//home", "patterns": ["**"]}]}}"#,
            "boundaries.zones[].root",
        ),
        (
            r#"{"boundaries": {"zones": [{"name": "a", "root": "././/home", "patterns": ["**"]}]}}"#,
            "boundaries.zones[].root",
        ),
        (
            r#"{"boundaries": {"zones": [{"name": "a", "autoDiscover": [".//home"]}]}}"#,
            "boundaries.zones[].autoDiscover",
        ),
        (r#"{"codeowners": ".//abs/CODEOWNERS"}"#, "codeowners"),
    ] {
        std::fs::write(&config, body).unwrap();
        let message = FallowConfig::load(&config).unwrap_err().to_string();
        assert!(message.contains(field), "{body}: {message}");
    }
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn find_config_path_refuses_a_config_link_that_leaves_the_project() {
    let dir = project_with_outside_file("outside.json", "{}");
    let project = dir.path().join("project");
    std::os::unix::fs::symlink(
        dir.path().join("outside.json"),
        project.join(".fallowrc.json"),
    )
    .unwrap();
    std::fs::write(project.join("fallow.toml"), "").unwrap();
    assert_eq!(FallowConfig::find_config_path(&project), None);
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn file_path_keys_are_refused_when_the_root_does_not_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let config: FallowConfig = serde_json::from_str(r#"{"codeowners": "CODEOWNERS"}"#).unwrap();
    let errors = config
        .check_file_path_keys_inside(&dir.path().join("missing-root"))
        .unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("codeowners"), "{errors:?}");
    let empty: FallowConfig = serde_json::from_str("{}").unwrap();
    assert!(
        empty
            .check_file_path_keys_inside(&dir.path().join("missing-root"))
            .is_ok()
    );
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn package_config_path_keys_may_name_files_up_to_the_git_top_level() {
    let dir = tempfile::tempdir().unwrap();
    let outer = dunce::canonicalize(dir.path()).unwrap();
    let top = outer.join("repo");
    let package = top.join("packages/app");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::create_dir_all(top.join(".git")).unwrap();
    std::fs::create_dir_all(top.join(".github")).unwrap();
    std::fs::write(top.join(".github/CODEOWNERS"), "* @owner\n").unwrap();
    let inside: FallowConfig = serde_json::from_str(
        r#"{"codeowners": "../../.github/CODEOWNERS",
            "health": {"coverage": "../../coverage/coverage-final.json"},
            "audit": {"deadCodeBaseline": "../../baselines/dead.json"}}"#,
    )
    .unwrap();
    assert!(inside.check_file_path_keys_inside(&package).is_ok());

    for body in [
        r#"{"codeowners": "../../../CODEOWNERS"}"#,
        r#"{"audit": {"dupesBaseline": "x/../../../../a.json"}}"#,
    ] {
        let config: FallowConfig = serde_json::from_str(body).unwrap();
        assert!(
            config.check_file_path_keys_inside(&package).is_err(),
            "{body}"
        );
    }
    std::fs::remove_dir(top.join(".git")).unwrap();
    assert!(inside.check_file_path_keys_inside(&package).is_err());
}

#[cfg(unix)]
fn git(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

#[cfg(unix)]
fn test_tracked_link_probe(path: &std::path::Path) -> Option<bool> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path.parent()?)
        .args(["--literal-pathspecs", "ls-files", "-z", "--"])
        .arg(path.file_name()?)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(!output.stdout.is_empty())
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn an_untracked_config_link_out_of_the_checkout_loads() {
    crate::set_tracked_link_probe(test_tracked_link_probe);
    let dir = tempfile::tempdir().unwrap();
    let outer = dunce::canonicalize(dir.path()).unwrap();
    let project = outer.join("project");
    std::fs::create_dir(&project).unwrap();
    git(&project, &["init", "-q"]);
    std::fs::write(outer.join("dotfiles.json"), r#"{"entry": ["src/a.ts"]}"#).unwrap();
    std::os::unix::fs::symlink(outer.join("dotfiles.json"), project.join(".fallowrc.json"))
        .unwrap();

    let (config, _) = FallowConfig::find_and_load(&project).unwrap().unwrap();
    assert_eq!(config.entry, vec!["src/a.ts".to_owned()]);
    assert!(FallowConfig::find_config_path(&project).is_some());

    git(&project, &["add", ".fallowrc.json"]);
    let message = FallowConfig::find_and_load(&project).unwrap_err();
    assert!(message.contains("symbolic link"), "{message}");
    assert!(FallowConfig::find_config_path(&project).is_none());
}
