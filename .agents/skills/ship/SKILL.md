---
name: ship
description: Land completed Fallow work after review, run pre-push parity, monitor merged-commit CI, and leave repositories clean. Use when asked to ship, merge, or move approved work to main.
---

# Ship

1. Confirm the branch and diff exactly match the reviewed scope.
2. Re-run the required checks from `docs/development/quality-gates.md`.
3. Verify companion commits, public contracts, and generated files are pushed.
4. Create signed conventional commits only.
5. Merge through the repository's current protected-main workflow.
6. Monitor the merged commit until required CI completes.
7. Inspect the merged tree for conflict markers and generated drift.
8. Return every touched checkout to a clean, synchronized state.

Do not call work complete while required merged-commit workflows are still
running.
