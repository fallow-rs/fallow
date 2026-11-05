---
name: fallow
description: Rust-native codebase analyzer for TypeScript and JavaScript projects.
agent-usage: Repository guide for Codex and other agents working on Fallow.
---

# Fallow agent guide

This file is the tracked, always-loaded router for Codex. Keep it short. Durable
architecture, validation, and review knowledge belongs under `docs/`, while
repeatable workflows belong under `.agents/skills/`.

## Start here

1. Read [`docs/README.md`](docs/README.md) for the maintained documentation map.
2. Use [`docs/development/task-context-map.md`](docs/development/task-context-map.md)
   to load only the references required for the task.
3. Load the matching workflow from `.agents/skills/<name>/SKILL.md`.
4. Follow nested `AGENTS.md` files when working in a scoped subsystem.

The shared development references are:

- [`docs/development/repo-map.md`](docs/development/repo-map.md)
- [`docs/development/quality-gates.md`](docs/development/quality-gates.md)
- [`docs/development/review-routing.md`](docs/development/review-routing.md)
- [`docs/development/ai-tooling.md`](docs/development/ai-tooling.md)

Do not rely on `.codex/skills`, sibling checkouts, private symlinks, or
machine-local files. A fresh clone must contain everything required by these
routes.

## Workflow

Fallow's native lifecycle is authoritative:

```text
implement -> panel-review -> review -> ship -> sweep
```

Run an objective from the maintainer through the full lifecycle in one pass.
Do not stop after `implement`, and do not wait for a prompt to ship or sweep.
Use `panel-review` only when the change has design or user-facing decisions.

An objective is done only when all of these are true:

- every pull request of the objective is merged to `main`, and the CI of the
  merged commit is complete and green (armed auto-merge is not enough);
- every companion pull request is merged to the default branch of its
  repository;
- every needed follow-up that the work found is fixed, or has a GitHub issue
  that the final report links (for a repository outside `fallow-rs`, a draft
  issue in the final report). An improvement idea with no known need goes
  under "Possible follow-ups" in the PR body, not in an issue;
- the touched branches and worktrees are clean.

A note in `.plans/`, in `/tmp`, or in the chat report does not track a
follow-up. An open pull request is not done. This is also true while CI runs.
When a merge is blocked, report the pull request, the blocker, and the next
action. Three stops are not blocks: a draft or no-merge request from the
maintainer, a maintainer step that a documented procedure requires (for
example the npm approval of a release), and an open decision for the
maintainer, reported with a recommended answer.

Use the corresponding tracked skills under `.agents/skills/`. Codex consumes
that canonical tree directly. Claude adapters under `.claude/skills/` are
generated and must pass `npm run check:agent-adapters`.

Before editing, check the current branch and worktree. Other agents may be
working in the same checkout. Re-read files immediately before editing, keep
changes narrowly scoped, and never revert unrelated work.

## Architecture

The analysis pipeline is:

```text
config -> discovery -> extract -> resolve -> graph -> analyze -> output
```

Use [`docs/development/repo-map.md`](docs/development/repo-map.md) for crate
ownership and high-value paths. Important invariants:

- Fallow performs syntactic analysis with Oxc, without the TypeScript compiler.
- Use `FxHashMap` and `FxHashSet` instead of the standard hash collections.
- Re-export chains and workspaces are first-class graph concerns.
- JSON, SARIF, CodeClimate, LSP, MCP, and editor output are public contracts.
- LSP and MCP should remain thin integration layers over shared analysis.

## Agent-facing CLI contract

- Use `--format json` and parse JSON for machine-readable output.
- Use `--quiet` to suppress progress output.
- Use issue filters to keep output and context bounded.
- Run `fallow fix --dry-run --format json --quiet` before any mutation.
- In a non-interactive process, apply an approved fix with `fix --yes`.
- Use `actions` arrays and `auto_fixable` instead of inventing fix behavior.
- Treat output paths as project-root-relative.
- Do not run the interactive `watch` command in agent workflows.
- Use `--explain` when metric interpretation is needed.
- Resolve current flags and commands from `fallow --help` and
  `fallow <command> --help`, not from copied flag catalogues.

Exit codes are `0` when no enforced gate failed, `1` when an enforced gate
failed (findings, not a tool failure), and `2` for invalid input or execution
errors. Bare `fallow` in a machine format exits 0 on findings unless you pass
`--fail-on-issues`. To gate, pass that flag or read `gate_outcomes`. Some
runtime workflows define additional documented exit codes.

## Validation

Run the smallest relevant checks first. The canonical repository commands are:

```bash
npm run verify:fast
npm run verify:full
```

See [`docs/development/quality-gates.md`](docs/development/quality-gates.md) for
the underlying commands, focused checks, and hook parity. Codex does not execute
Claude hooks automatically.

Any bug fix needs:

1. a minimal reproduction that fails without the fix,
2. validation against a real user project or public fixture,
3. the relevant full suite.

If a step cannot be run, state that explicitly instead of presenting partial
evidence as complete.

## Write for the reader

Write code comments, docs, commit messages, PR text, and changelog entries
for a reader who did not see the session. Do not put the conversation, the
prompt, discarded attempts, or the edit history into them. Describe the code
as it is now. For a regression, state what breaks without the code, and link
the issue. `scripts/check-comment-quality.mjs` blocks narration and history
comments in commits, CI, and Claude edits.

Before you open or update a pull request, read each changed comment, doc,
commit message, and PR text as a reader with no access to the session.
Remove each reference to the session. Explain each term that a new reader
does not know.

## Knowledge and publication boundaries

- This repository owns open-source maintainer knowledge and public product
  contracts.
- `fallow-docs` owns authored public user documentation.
- This repository owns the released Fallow skill contract.
- `fallow-skills` consumes that contract and owns portable plugin packaging
  plus additional public end-user skills.
- Private repositories may consume pinned public artifacts and contracts.
- Never automate copying private prose into this repository or another public
  artifact. Public promotion is a deliberate, reviewed rewrite.
- Never publish output from private or client projects. Use generic examples.

The complete ownership and synchronization model is documented in
[`docs/development/knowledge-architecture.md`](docs/development/knowledge-architecture.md).

## Git

- Use conventional commit prefixes.
- Sign commits with `git commit -S -s`. The `-s` flag adds the DCO sign-off that CI requires.
- Do not add AI attribution or `Co-Authored-By` trailers.
- Before committing, verify `git status` and the exact staged paths.
- Mirror the `hk.pkl` pre-commit and pre-push checks when hooks did
  not run.
