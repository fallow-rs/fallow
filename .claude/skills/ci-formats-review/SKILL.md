---
name: ci-formats-review
description: Review SARIF, CodeClimate, compact, markdown, badge, and other CI-facing output formats for correctness and integrator expectations. Use when changes affect machine-consumed report formats or CI presentation layers.
---
<!-- Generated from .agents/skills. Do not edit. -->

# CI Formats Review

Read `.agents/agents/ci-formats-reviewer.md` before reviewing.

Also read `.agents/rules/testing.md` when snapshots or integration tests are involved.

Focus on:
- format correctness
- compatibility with CI consumers
- empty-input handling
- stable snapshots and ordering

End with `APPROVE`, `CONCERN`, or `BLOCK`.
