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
