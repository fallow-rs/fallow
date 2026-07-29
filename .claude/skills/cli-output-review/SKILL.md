---
name: cli-output-review
description: Review fallow's human-readable CLI output for scanability, information hierarchy, empty states, and terminal compatibility. Use when changes affect human output, command UX, or text formatting in the CLI.
---
<!-- Generated from .agents/skills. Do not edit. -->

# CLI Output Review

Read:
- `.agents/agents/cli-output-reviewer.md`
- `.agents/rules/cli-crate.md`

Review the concrete output path under `crates/cli/` and any related snapshots or tests.

Focus on:
- information hierarchy
- scanability
- progressive disclosure
- empty states
- `NO_COLOR` and terminal compatibility

End with `APPROVE`, `CONCERN`, or `BLOCK`.
