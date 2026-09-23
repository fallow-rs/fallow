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
4. Resolve open decisions before editing. Sort each open question:
   - fact: the code, docs or history hold the answer; look it up;
   - observable: a run shows the answer; probe a fresh build on a public
     fixture with `--format json --quiet`;
   - decision: scope, naming, contract or trade-off; ask the maintainer in one
     round, with a recommended answer for each question.
   Record the answers in the plan. Skip this step when no decision is open.
5. For changes with user-facing design decisions, run `panel-review` before
   editing.
6. Create an implementation branch and ready pull request before tracked edits.
7. Implement in pipeline order and keep generated contracts, public docs, and
   companion repositories synchronized. Follow the test evidence rules in
   `docs/development/quality-gates.md`. For a bug, get a command that fails
   before you change code; after a restart, suspect the `.fallow/` cache first.
8. Run every applicable gate from `docs/development/quality-gates.md`.
9. Run the reviewer set selected by
   `docs/development/review-routing.md`. Resolve every block.
10. Update the pull request with current verification evidence and the proof
    level of each claim.

Preserve unrelated work. Never create process artifacts under committed
`docs/`. Prefer durable knowledge in indexed docs and keep this skill
procedural.
