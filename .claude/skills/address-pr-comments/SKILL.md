---
name: address-pr-comments
description: Triage and address GitHub PR review feedback for fallow, then implement the agreed fixes. Use when the user wants to inspect PR comments, requested changes, or unresolved review threads and act on them.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Address PR Comments

Use GitHub review metadata plus the local checkout.

Workflow:
1. Fetch PR metadata and review comments. Confirm that the local branch is at
   the PR head SHA.
2. Check each comment against the code at the PR head before any change.
   Trace the call sites for a "this can fail" claim. Give each check an
   evidence level from `docs/development/quality-gates.md`.
3. Sort each comment into one group:
   - Act on: the check shows the problem. Only these comments change code.
   - Consider: valid, but it has a cost or changes scope. Ask the maintainer.
   - Noted: true, but no change is necessary now.
   - Dismissed: wrong, a preference, or missing context. Give one reason.
4. Implement the Act on fixes, one comment at a time.
5. Run the smallest relevant validation.
6. Draft one reply per thread for the maintainer to approve. Never post a
   reply or resolve a thread. Lead with the fix and its commit. Thank a
   contributor in one short sentence.
7. Summarize each group, with the reason for each Dismissed comment.

Follow repo conventions from `AGENTS.md`: no AI attribution, signed commits when committing.
