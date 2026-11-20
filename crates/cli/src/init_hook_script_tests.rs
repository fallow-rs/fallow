//! Tests for the shell text of the pre-commit hook, the existing-hook hint
//! and the Lefthook job that `fallow hooks install --target git` writes.

use super::{
    RESOLVE_FALLOW_BLOCK, build_pre_commit_hook_content, existing_hook_hint, lefthook_hint,
};

#[test]
fn existing_hook_hint_includes_resolution_block() {
    let hint = existing_hook_hint(".git/hooks/pre-commit", false);
    assert!(hint.contains(".git/hooks/pre-commit already exists"));
    assert!(hint.contains("command -v fallow"));
    assert!(hint.contains("  run_fallow audit --staged --quiet --gate-marker pre-commit"));
    assert!(!hint.contains("--base"));
}

#[test]
fn lefthook_hint_includes_resolution_block() {
    let hint = lefthook_hint(false);
    assert!(hint.contains("Lefthook detected"));
    assert!(hint.contains("pre-commit:"));
    assert!(hint.contains("run: |"));
    assert!(hint.contains("./node_modules/.bin/fallow"));
    assert!(hint.contains("yarn exec fallow -- \"$@\""));
    assert!(hint.contains("run_fallow audit --staged --quiet --gate-marker pre-commit"));
    assert!(!hint.contains("--base"));
}

/// The staged audit needs a commit at HEAD. Every hook form keeps the first
/// commit of a repository and names the skip.
#[test]
fn hook_templates_skip_the_audit_before_the_first_commit() {
    for text in [
        build_pre_commit_hook_content(false),
        existing_hook_hint(".git/hooks/pre-commit", false),
        lefthook_hint(false),
    ] {
        let guard = text
            .find("git rev-parse --verify --quiet HEAD")
            .expect("the hook checks HEAD");
        let audit = text.find("audit --staged").expect("the hook audits");
        assert!(guard < audit, "{text}");
    }
}

/// The standalone hook, the existing-hook hint and the Lefthook job must
/// resolve the same installs, so each carries the shared block.
#[test]
fn hook_templates_share_the_resolution_block() {
    let unindent = |text: &str| -> String {
        text.lines()
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let block = unindent(RESOLVE_FALLOW_BLOCK);
    for (name, text) in [
        ("hook", build_pre_commit_hook_content(false)),
        (
            "existing hook hint",
            existing_hook_hint(".git/hooks/pre-commit", false),
        ),
        ("lefthook hint", lefthook_hint(false)),
    ] {
        assert!(unindent(&text).contains(&block), "{name}: {text}");
    }
    assert!(RESOLVE_FALLOW_BLOCK.contains("npx --no-install fallow --version"));
    assert!(RESOLVE_FALLOW_BLOCK.contains("skipping the pre-commit audit"));
}

#[test]
fn prune_baselines_hook_prunes_before_the_audit() {
    let content = build_pre_commit_hook_content(true);
    let prune = content
        .find("fallow baselines prune --quiet")
        .expect("the hook runs the prune");
    let audit = content
        .find("fallow audit --staged")
        .expect("the hook runs the audit");
    assert!(prune < audit, "{content}");
    assert!(content.contains("*next-index-*)"));
    assert!(content.contains("git ls-files --others --exclude-standard"));
    assert!(content.contains("elif run_fallow baselines prune --quiet; then\n      git add -u"));
    assert!(!build_pre_commit_hook_content(false).contains("baselines prune"));
}

#[test]
fn prune_baselines_hints_carry_the_prune_block() {
    let hint = existing_hook_hint(".git/hooks/pre-commit", true);
    assert!(
        hint.contains("\n      elif run_fallow baselines prune --quiet; then"),
        "{hint}"
    );
    let lefthook = lefthook_hint(true);
    assert!(
        lefthook.contains("\n              elif run_fallow baselines prune --quiet; then"),
        "{lefthook}"
    );
    assert!(!lefthook_hint(false).contains("baselines prune"));
}

/// Runs the resolution block in `project` with `PATH` reduced to
/// `project/bin` plus the system directories. Each stub prints the given
/// `fallow --version` line. The script prints the version of the runner that
/// the block selected.
#[cfg(unix)]
fn run_resolution_block(
    path_version: Option<&str>,
    local_version: Option<&str>,
) -> std::process::Output {
    run_resolution_block_with_path(path_version, local_version, "/usr/bin:/bin")
}

/// [`run_resolution_block`] with `system_path` after `project/bin` on `PATH`.
/// An empty `system_path` gives the hook no external command, as in a Git
/// client with a very small `PATH`.
#[cfg(unix)]
fn run_resolution_block_with_path(
    path_version: Option<&str>,
    local_version: Option<&str>,
    system_path: &str,
) -> std::process::Output {
    use std::os::unix::fs::PermissionsExt;

    let write_stub = |path: &std::path::Path, version: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Each line of `fallow {version}` becomes one output line, so a stub
        // can print the `verified:` line of the npm launcher after the version.
        let lines: Vec<String> = format!("fallow {version}")
            .lines()
            .map(|line| format!("'{line}'"))
            .collect();
        std::fs::write(
            path,
            format!("#!/bin/sh\nprintf '%s\\n' {}\n", lines.join(" ")),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    std::fs::create_dir_all(project.join("bin")).unwrap();
    if let Some(version) = path_version {
        write_stub(&project.join("bin/fallow"), version);
    }
    if let Some(version) = local_version {
        write_stub(&project.join("node_modules/.bin/fallow"), version);
    }
    let script = project.join("resolve.sh");
    std::fs::write(
        &script,
        format!("{RESOLVE_FALLOW_BLOCK}run_fallow --version\n"),
    )
    .unwrap();

    // An absolute shell path: the reduced `PATH` may not hold `sh`.
    std::process::Command::new("/bin/sh")
        .arg(&script)
        .current_dir(project)
        .env(
            "PATH",
            format!("{}:{system_path}", project.join("bin").display()),
        )
        .output()
        .expect("spawn sh")
}

/// A project pins its fallow version in its dependencies. A global install
/// on `PATH` can have another version and another packaging, so the hook
/// must run the project-local launcher first.
#[cfg(unix)]
#[test]
fn resolution_block_prefers_the_project_local_launcher_over_path() {
    let output = run_resolution_block(Some("9.0.0"), Some("9.1.0"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "fallow 9.1.0", "{output:?}");
}

/// `audit --staged` exists since fallow 3.43.0. An older fallow rejects the
/// flag with a parser error, so the hook blocks with a message that names
/// the runner and the fix.
#[cfg(unix)]
#[test]
fn resolution_block_blocks_a_local_fallow_below_the_hook_floor() {
    let output = run_resolution_block(Some("9.0.0"), Some("3.42.9"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr.contains(
            "fallow-hook: blocked: ./node_modules/.bin/fallow is fallow 3.42.9, below the required 3.43.0."
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("update the fallow devDependency"),
        "{stderr}"
    );
}

/// The npm launcher prints a `verified:` line after the version line. The
/// block message names only the version.
#[cfg(unix)]
#[test]
fn resolution_block_reads_only_the_first_version_line() {
    // No system directory on `PATH`: the block must use shell builtins only.
    let output = run_resolution_block_with_path(
        None,
        Some("3.40.0\nverified: at ./node_modules/.bin/fallow"),
        "",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr.contains(
            "fallow-hook: blocked: ./node_modules/.bin/fallow is fallow 3.40.0, below the required 3.43.0.\n"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("verified:"), "{stderr}");
}

/// A fallow on `PATH` below the floor gets the hint to upgrade that install.
#[cfg(unix)]
#[test]
fn resolution_block_blocks_a_path_fallow_below_the_hook_floor() {
    let output = run_resolution_block(Some("2.85.0"), None);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr.contains("is fallow 2.85.0, below the required 3.43.0."),
        "{stderr}"
    );
    assert!(stderr.contains("upgrade the fallow on PATH"), "{stderr}");
}

/// The floor itself and a newer major version pass.
#[cfg(unix)]
#[test]
fn resolution_block_accepts_a_fallow_at_or_above_the_hook_floor() {
    for version in ["3.43.0", "4.0.0"] {
        let output = run_resolution_block(None, Some(version));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), format!("fallow {version}"), "{output:?}");
    }
}
