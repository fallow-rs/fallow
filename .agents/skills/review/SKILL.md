---
name: review
description: Perform Fallow's comprehensive pre-merge review. Use after implementation or when asked to review a branch, pull request, or diff.
---

# Review

1. Re-read `AGENTS.md`, the active plan, the current branch, and the complete
   diff against its intended base.
2. Run the applicable gates in `docs/development/quality-gates.md`.
3. Select reviewers using `docs/development/review-routing.md`.
4. Review public contracts, all affected output formats, filters, integrations,
   generated surfaces, security boundaries, and companion parity.
5. Run a behavior-facing smoke on a real project when runtime behavior changed.
6. Classify each result as `APPROVE`, `CONCERN`, or `BLOCK`.
7. Fix every block, rerun the blocking reviewer, and update verification.

Review the actual source and live output. Do not infer correctness from a green
compile, a narrow unit test, or a previously cached report.
