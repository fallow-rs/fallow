use std::process::ExitCode;

use fallow_engine::clear_ambient_git_env;

use crate::error::emit_error;

use super::AuditOptions;

/// A base ref resolved by auto-detection: the git ref to diff against plus a
/// human-readable provenance string for the scope line.
pub struct DetectedBase {
    /// The ref the audit diffs against: a `git merge-base` SHA (the fork
    /// point), a remote-tracking ref, or a local branch name.
    pub git_ref: String,
    /// How the ref was resolved, e.g. `merge-base with origin/main`. Shown on
    /// the human audit scope line so the comparison target is checkable.
    pub description: String,
}

/// Run `git <args>` in `root` with ambient git env cleared and return trimmed
/// stdout, or `None` on non-zero exit / empty output.
fn git_stdout(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Whether `git_ref` resolves to a commit in this repository.
fn git_ref_exists(root: &std::path::Path, git_ref: &str) -> bool {
    git_stdout(root, &["rev-parse", "--verify", "--quiet", git_ref]).is_some()
}

/// The current branch's configured upstream (`@{upstream}`), e.g. `origin/main`,
/// or `None` when no tracking branch is set (detached HEAD, fresh worktree).
fn git_upstream_ref(root: &std::path::Path) -> Option<String> {
    git_stdout(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
}

/// The merge-base (fork point) SHA of `a` and `b`, or `None` when there is no
/// common ancestor (shallow clone, unrelated history).
fn git_merge_base(root: &std::path::Path, a: &str, b: &str) -> Option<String> {
    git_stdout(root, &["merge-base", a, b])
}

/// The remote default branch as a remote-tracking ref (`origin/<branch>`).
/// Priority: `origin/HEAD` symbolic ref, then `origin/main`, then
/// `origin/master`. Returns `None` when there is no `origin` remote at all.
fn detect_remote_default_ref(root: &std::path::Path) -> Option<String> {
    if let Some(full_ref) = git_stdout(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/")
    {
        return Some(format!("origin/{branch}"));
    }
    for candidate in ["origin/main", "origin/master"] {
        if git_ref_exists(root, candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Auto-detect the base ref for `fallow audit` when no `--base` / env override
/// is set.
///
/// The base is the `git merge-base` (fork point) against the branch's upstream
/// or the remote default, mirroring the `fallow hooks install --target git`
/// pre-commit hook (issue #242). Resolving to the merge-base SHA, rather than a
/// bare branch name, fixes the long-standing bug where the default branch was
/// discovered via `origin/HEAD` but returned as the bare name `main` (issue
/// #1168): git resolves a bare `main` to the LOCAL `refs/heads/main`, which is
/// stale on worktree checkouts cut from `origin/main`, so the audit diffed
/// every branch against an ancient base and false-failed the gate.
///
/// Resolution order:
/// 1. `@{upstream}` merge-base, so a branch forked off a non-default
///    integration branch compares against where it actually forked.
/// 2. Remote default (`origin/HEAD` -> `origin/main` -> `origin/master`)
///    merge-base. The remote-tracking ref refreshes on fetch, unlike a
///    long-stale local branch; the merge-base is also immune to an unfetched
///    `origin/main` in the false-fail direction.
/// 3. Local `main` / `master` when there is no `origin` remote, preserving the
///    historical behavior for air-gapped / local-only repos.
pub fn auto_detect_base_ref(root: &std::path::Path) -> Option<DetectedBase> {
    if let Some(upstream) = git_upstream_ref(root) {
        if let Some(sha) = git_merge_base(root, &upstream, "HEAD") {
            return Some(DetectedBase {
                git_ref: sha,
                description: format!("merge-base with {upstream}"),
            });
        }
        // No common ancestor (shallow clone / unrelated history): fall back to
        // the upstream tip rather than failing the detection outright.
        return Some(DetectedBase {
            description: format!("{upstream} (tip)"),
            git_ref: upstream,
        });
    }

    if let Some(remote_ref) = detect_remote_default_ref(root) {
        if let Some(sha) = git_merge_base(root, &remote_ref, "HEAD") {
            return Some(DetectedBase {
                git_ref: sha,
                description: format!("merge-base with {remote_ref}"),
            });
        }
        return Some(DetectedBase {
            description: format!("{remote_ref} (tip)"),
            git_ref: remote_ref,
        });
    }

    for candidate in ["main", "master"] {
        if git_ref_exists(root, candidate) {
            return Some(DetectedBase {
                git_ref: candidate.to_string(),
                description: format!("local {candidate}"),
            });
        }
    }

    None
}

/// Get the short SHA of HEAD for the scope display line.
pub fn get_head_sha(root: &std::path::Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Parse a raw `FALLOW_AUDIT_BASE` value: trim, treat empty / whitespace-only as
/// unset. Pure helper so the trimming logic is testable without mutating env.
pub fn parse_audit_base_override(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// The `FALLOW_AUDIT_BASE` override (trimmed), or `None` when unset / empty.
/// Lets a downstream consumer pin the base without editing the generated agent
/// gate script (issue #1168), e.g. `FALLOW_AUDIT_BASE=upstream/main` on a fork.
fn audit_base_env_override() -> Option<String> {
    parse_audit_base_override(std::env::var("FALLOW_AUDIT_BASE").ok())
}

/// Resolve the base ref and an optional human-readable provenance for the scope
/// line. Precedence: explicit `--changed-since` / `--base` flag, then the
/// `FALLOW_AUDIT_BASE` env override, then auto-detection.
pub fn resolve_base_ref(opts: &AuditOptions<'_>) -> Result<(String, Option<String>), ExitCode> {
    if let Some(ref_str) = opts.changed_since {
        return Ok((ref_str.to_string(), None));
    }
    if let Some(env_ref) = audit_base_env_override() {
        if let Err(e) = crate::validate::validate_git_ref(&env_ref) {
            return Err(emit_error(
                &format!("FALLOW_AUDIT_BASE='{env_ref}' is not a valid git ref: {e}"),
                2,
                opts.output,
            ));
        }
        let description = format!("FALLOW_AUDIT_BASE={env_ref}");
        return Ok((env_ref, Some(description)));
    }
    let Some(detected) = auto_detect_base_ref(opts.root) else {
        return Err(emit_error(
            "could not detect base branch. Use --base <ref> to specify the comparison target (e.g., --base main)",
            2,
            opts.output,
        ));
    };
    if let Err(e) = crate::validate::validate_git_ref(&detected.git_ref) {
        return Err(emit_error(
            &format!(
                "auto-detected base ref '{}' is not a valid git ref: {e}",
                detected.git_ref
            ),
            2,
            opts.output,
        ));
    }
    Ok((detected.git_ref, Some(detected.description)))
}
