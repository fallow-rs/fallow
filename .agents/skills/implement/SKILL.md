---
name: implement
description: Research, implement, test, document, and review a Fallow feature, fix, refactor, or repository improvement. Use when asked to build or change Fallow.
---

# Implement

Deliver the requested change through the native Fallow lifecycle.

1. Read `AGENTS.md`, `docs/README.md`, and
   `docs/development/task-context-map.md`.
2. Inspect the live branch, worktree, open pull requests, and `origin/main`.
   When an open pull request of another author or session already makes the
   same change, do not open a competing one. Report its URL to the
   maintainer as an open decision, with a recommended answer. A pull request
   of this objective does not count.
   Search with the key terms of the change, not only the issue number,
   because a pull request can have no link to the issue. Use
   `gh pr list --state all --search "<terms>"`. Do the search and a rebase
   on `origin/main` again before you open the pull request, because another
   session can merge the same change in the meantime.
3. Write the acceptance criteria and verification plan to the gitignored
   `.plans/<task>.md`. Write the logs, builds, clones, probes and drafts of
   the task to `.plans/<task>/`, not to the top of `.plans/`.
4. Resolve open decisions before editing. Sort each open question:
   - fact: the code, docs or history hold the answer; look it up;
   - observable: a run shows the answer; probe a fresh build on a public
     fixture with `--format json --quiet`. When the change models another
     tool (pnpm, npm, Bun, tsc), probe that tool too: the oldest supported
     version, the installed one and the latest major. Narrow a version
     boundary down to the patch release;
   - decision: scope, naming, contract or trade-off; ask the maintainer in one
     round, with a recommended answer for each question.
   Record the answers in the plan. Skip this step when no decision is open.
5. For changes with user-facing design decisions, run `panel-review` before
   editing.
6. Create an implementation branch and ready pull request before tracked edits.
   Record the URL of each pull request of the objective, in every
   repository, in `.plans/<task>.md` when you open it. `sweep` checks this
   list.
7. Implement in pipeline order and keep generated contracts, public docs, and
   companion repositories synchronized. Follow the test evidence rules in
   `docs/development/quality-gates.md`. For a bug, get a command that fails
   before you change code; after a restart, suspect the `.fallow/` cache first.
   Write changelog entries to `changelog.d/<branch>.md`, never to
   `CHANGELOG.md`.
8. Run every applicable gate from `docs/development/quality-gates.md`.
9. Run the reviewer set selected by
   `docs/development/review-routing.md`. Resolve every block.
10. Update the pull request with current verification evidence and the proof
    level of each claim.
11. Continue with `review`, then `ship`, then `sweep` in the same session.
    Do not stop at an open pull request, and do not wait for a prompt. Stop
    only for a decision that belongs to the maintainer. The work is done only
    when it meets the definition of done in `AGENTS.md`.

Preserve unrelated work. Never create process artifacts under committed
`docs/`. Prefer durable knowledge in indexed docs and keep this skill
procedural.
