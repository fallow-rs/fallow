---
name: json-output-review
description: Review machine-readable JSON output changes for schema stability, determinism, and downstream usability. Use when changes affect JSON output, result fields, ordering, snapshots, or integration contracts.
---

# JSON Output Review

Read `.agents/agents/json-output-reviewer.md` before reviewing.

Also check any related snapshots, schema files, and output docs.

Focus on:
- compatibility and drift risk
- stable ordering
- null/empty behavior
- field naming consistency
- downstream automation friendliness

End with `APPROVE`, `CONCERN`, or `BLOCK`.
