---
name: triage-issue
description: Triage a GitHub issue against the current fallow codebase and determine validity, priority, scope, and likely implementation shape. Use when the user asks to triage an issue, assess a bug report, or evaluate an enhancement request.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Triage Issue

Use the GitHub connector and local repo inspection together.

Workflow:
1. Fetch the issue and comments.
2. Reproduce the report on a binary built from current `main` before you
   judge it. A report against an older release can be fixed already.
3. Inspect the relevant code paths locally.
4. Determine whether the issue is valid, overstated, underspecified, or already covered.
5. Recommend priority and whether the issue should be kept, split, narrowed, or closed.
6. Give acceptance criteria tied to the current codebase.

Give each claim about cause or history one confidence level:
- direct: you ran it, or the source shows it;
- supported: two independent sources agree;
- inferred: it follows from the evidence, but nothing shows it;
- speculative: plausible, with no evidence;
- unknown.

Write "because" only with a source: a `file:line`, a commit, or command
output. Code does not prove the intent of its author.

When useful, pull in:
- `rust-review`
- `panel-review`
- `team-assembly`

Always cite local file references for implementation claims.
