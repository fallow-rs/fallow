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
5. When runtime behavior changed, run the behavior comparison on public
   projects from `docs/development/quality-gates.md` and explain every
   difference.
6. Filter reviewer findings before you act on them:
   - trace the call site before you accept a bug claim;
   - drop style preferences and "a different design would also work";
   - check the diff or output that a reviewer cites, not its summary;
   - mark each accepted claim with its proof level.
   Record dismissed findings with a one-line reason in the verification notes.
   More than about five blocks usually means the filter is too weak.
7. Classify each result as `APPROVE`, `CONCERN`, or `BLOCK`.
8. Fix every block, rerun the blocking reviewer, and update verification.

Review the actual source and live output. Do not infer correctness from a green
compile, a narrow unit test, or a previously cached report.
