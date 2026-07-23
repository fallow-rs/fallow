---
name: panel-review-loop
description: Iteratively review and improve a Fallow user-facing surface across representative real-world projects until the panel has no blocking concerns.
---

# Panel review loop

1. Define the user-visible surface and success criteria.
2. Select representative projects from `benchmarks/fixtures/real-world/`.
3. Capture actual output for each project.
4. Run `panel-review` on the evidence, not on an intended answer.
5. Implement the smallest coherent improvement that addresses consensus.
6. Re-run the same corpus and compare behavior.
7. Stop when the panel has no blocks and further changes do not materially
   improve the surface.

Keep the corpus stable across iterations. Preserve output contracts unless the
plan explicitly approves a versioned change.
