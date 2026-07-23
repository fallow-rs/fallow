---
name: sig-audit-loop
description: Iteratively improve Fallow maintainability using measured SIG audit deltas, retaining only changes that improve the targeted property without regressions.
---
<!-- Generated from .agents/skills. Do not edit. -->

# SIG audit loop

1. Run `sig-audit` and select one weak property.
2. Identify a bounded structural change with a measurable expected effect.
3. Measure before, implement, and measure after with identical settings.
4. Keep the change only when the target improves and verification remains
   green.
5. Repeat until the target is met or further changes have no credible benefit.
6. Run `review` before proposing the final result.

Never combine unrelated cleanup into one measurement iteration.
