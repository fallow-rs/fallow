---
name: triage-issue
description: Triage a GitHub issue against the current fallow codebase and determine validity, priority, scope, and likely implementation shape. Use when the user asks to triage an issue, assess a bug report, or evaluate an enhancement request.
---

# Triage Issue

Use the GitHub connector and local repo inspection together.

Workflow:
1. Fetch the issue and comments.
2. Inspect the relevant code paths locally.
3. Determine whether the issue is valid, overstated, underspecified, or already covered.
4. Recommend priority and whether the issue should be kept, split, narrowed, or closed.
5. Give acceptance criteria tied to the current codebase.

When useful, pull in:
- `rust-review`
- `panel-review`
- `team-assembly`

Always cite local file references for implementation claims.
