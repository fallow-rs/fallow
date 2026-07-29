---
name: address-pr-comments
description: Triage and address GitHub PR review feedback for fallow, then implement the agreed fixes. Use when the user wants to inspect PR comments, requested changes, or unresolved review threads and act on them.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Address PR Comments

Use GitHub review metadata plus the local checkout.

Workflow:
1. Fetch PR metadata and review comments.
2. Separate actionable comments from noise or already-resolved remarks.
3. Inspect the touched code paths locally.
4. Implement the agreed fixes.
5. Run the smallest relevant validation.
6. Summarize what was addressed and any comments left intentionally unresolved.

Follow repo conventions from `AGENTS.md`: no AI attribution, signed commits when committing.
