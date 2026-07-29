---
name: github-action-review
description: Review fallow's GitHub Action, shell scripts, jq filters, annotations, review comments, and workflow integration. Use when changes touch action/, action.yml, GitHub review formatting, or CI shell/jq behavior.
---

# GitHub Action Review

Read:
- `.agents/agents/github-action-reviewer.md`
- `.agents/rules/testing.md`

Review the actual changed scripts, jq filters, action definition, and tests.

Prioritize:
- shell safety
- token handling
- jq robustness on empty and malformed data
- annotation/comment correctness
- idempotent PR behavior

End with `APPROVE`, `CONCERN`, or `BLOCK`.
