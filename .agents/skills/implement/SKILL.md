---
name: implement
description: Research, implement, test, document, and review a Fallow feature, fix, refactor, or repository improvement. Use when asked to build or change Fallow.
---

# Implement

Deliver the requested change through the native Fallow lifecycle.

1. Read `AGENTS.md`, `docs/README.md`, and
   `docs/development/task-context-map.md`.
2. Inspect the live branch, worktree, open pull requests, and `origin/main`.
3. Write the acceptance criteria and verification plan to the gitignored
   `.plans/<task>.md`.
4. For changes with user-facing design decisions, run `panel-review` before
   editing.
5. Create an implementation branch and ready pull request before tracked edits.
6. Implement in pipeline order and keep generated contracts, public docs, and
   companion repositories synchronized.
7. Run every applicable gate from `docs/development/quality-gates.md`.
8. Run the reviewer set selected by
   `docs/development/review-routing.md`. Resolve every block.
9. Update the pull request with current verification evidence.

Use `apply_patch` for edits. Preserve unrelated work. Never create process
artifacts under committed `docs/`. Prefer durable knowledge in indexed docs and
keep this skill procedural.
