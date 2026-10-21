---
name: open-draft-pr
description: Prepare local changes for review with an intentional commit, push, and ready PR for fallow. Use when the user wants to publish work, open a PR, or turn local changes into a reviewable branch.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Open Review PR

This skill keeps its historical file/name for compatibility, but fallow PRs are
ready for review by default. Do not open draft PRs unless the user explicitly
asks for a draft.

Workflow:
1. Review the changed files and confirm the scope.
2. Run the smallest relevant validation for the touched area.
3. Prepare a concise conventional commit message.
4. Commit with `-S -s` (signature and DCO sign-off) if committing is requested.
5. Push the branch. GitHub refuses a PR for a branch with no commits; make an
   intentional empty coordination commit before the first push when the
   branch is fresh.
6. Write the PR body to a file and run the CI deferral check on it:
   `PR_BODY="$(cat body.md)" GITHUB_REPOSITORY=fallow-rs/fallow node .github/scripts/check-pr-deferrals.mjs`.
   The check fails a body that defers work without an issue link, for
   example a "Follow-up" or "Not in this change" section. Link an issue,
   or add "No follow-up planned." to an intentional non-goal. Call a docs
   change a companion fallow-docs PR, not a follow-up.
7. Open a ready PR with a high-signal conventional title and that body.
   After a later body edit, do not rerun the failed commit-message job:
   the `edited` event starts a new run with the new body.

Requirements:
- Use conventional commits.
- Use signed commits with a DCO sign-off: `git commit -S -s`.
- Use a descriptive non-`codex/` branch name, for example `feat/<slug>`,
  `fix/<slug>`, `refactor/<slug>`, `docs/<slug>`, or `chore/<slug>`.
- Do not pass `--draft` to `gh pr create` unless the user explicitly asks for a draft.
- Do not add AI attribution.
- Summarize validation honestly if some checks were not run.
