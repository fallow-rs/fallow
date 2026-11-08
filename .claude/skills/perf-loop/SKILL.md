---
name: perf-loop
description: Iteratively optimize Fallow performance with stable benchmarks, before-and-after evidence, and correctness gates.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Performance loop

1. Choose a stable benchmark and preserve its identity and workload. Before
   you freeze it, prove that it can see the problem: a known slowdown must
   move the result.
2. Record a statistically useful baseline. Set a target and a minimum number
   of rounds. Do not stop before that minimum unless you reach the target.
3. Profile the hot path before editing.
4. Implement one bounded optimization.
5. Re-run the same benchmark and correctness checks. Run the base binary
   and the candidate as interleaved pairs (base, candidate, base, and so
   on), and record the host load with each run. Other work on the host can
   change one run by more than the gain. Under load, compare CPU time, not
   wall time.
6. Keep the change only when the improvement is reproducible and no contract
   regresses.
7. Use a new benchmark identifier for a materially different workload.
8. Run `review`.

Do not report performance gains from debug builds or incomparable fixtures.
