---
name: perf-loop
description: Iteratively optimize Fallow performance with stable benchmarks, before-and-after evidence, and correctness gates.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Performance loop

1. Choose a stable benchmark and preserve its identity and workload.
2. Record a statistically useful baseline.
3. Profile the hot path before editing.
4. Implement one bounded optimization.
5. Re-run the same benchmark and correctness checks.
6. Keep the change only when the improvement is reproducible and no contract
   regresses.
7. Use a new benchmark identifier for a materially different workload.
8. Run `review`.

Do not report performance gains from debug builds or incomparable fixtures.
