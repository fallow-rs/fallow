---
name: sweep
description: Audit the current Fallow session for missed work, incomplete verification, stale documentation, companion drift, or cleanup before final completion.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Sweep

In this repository, use this sweep instead of a general sweep workflow.

1. Re-read the user's full request, active plan, diff, and live pull-request
   state.
2. Map every acceptance criterion to authoritative evidence.
3. Check for missed consumers, output formats, filters, docs, skills, schemas,
   CI scripts, companion repositories, and private/public boundaries.
4. Re-run any weak or stale verification.
5. Fix in-scope omissions and document genuine separate follow-ups.
6. Verify branches, worktrees, temporary worktrees, and generated artifacts are
   clean.
7. List the lessons of the task: corrections, failures that cost time, and
   skipped steps. Keep a lesson only when all four filters pass:
   - it stays true after paths, versions, and counts change;
   - it changes a future decision, not only the amount of text to read;
   - no skill or doc covers it yet (sharpen a weak rule, do not copy it);
   - it belongs to a skill, doc, or surface that this task used.
8. Store each kept lesson in the strongest form: a type or schema, then a
   lint, test, or CI check, then a script. Write text only when none fits.

Finding no obvious bug is not completion. Prove every requested outcome.
