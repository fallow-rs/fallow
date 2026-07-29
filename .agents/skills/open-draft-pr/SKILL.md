---
name: open-draft-pr
description: Prepare local changes for review with an intentional commit, push, and ready PR for fallow. Use when the user wants to publish work, open a PR, or turn local changes into a reviewable branch.
---

# Open Review PR

This skill keeps its historical file/name for compatibility, but fallow PRs are
ready for review by default. Do not open draft PRs unless the user explicitly
asks for a draft.

Workflow:
1. Review the changed files and confirm the scope.
2. Run the smallest relevant validation for the touched area.
3. Prepare a concise conventional commit message.
4. Commit with signing if committing is requested.
5. Push the branch.
6. Open a ready PR with a high-signal conventional title and body.

Requirements:
- Use conventional commits.
- Use signed commits.
- Use a descriptive non-`codex/` branch name, for example `feat/<slug>`,
  `fix/<slug>`, `refactor/<slug>`, `docs/<slug>`, or `chore/<slug>`.
- Do not pass `--draft` to `gh pr create` unless the user explicitly asks for a draft.
- Do not add AI attribution.
- Summarize validation honestly if some checks were not run.
