---
name: debug-false-positive
description: Diagnose and fix a Fallow false positive or false negative through extraction, resolution, graph, analysis, reporting, and real-consumer verification.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Debug a false result

1. Reproduce on current `main` with `--format json --quiet`.
2. Reduce to a minimal fixture without losing the behavior.
3. Trace the finding through extract, resolve, graph, reachability, analysis,
   suppression, filters, and report assembly.
4. Fix the earliest incorrect layer.
5. Add a regression test that fails without the fix.
6. Run the fixed binary on a real consumer project with a cleared `.fallow/`
   cache and compare old versus new output.
7. Run `review` with the affected surface reviewers.

Do not tune expected output around one fixture when the semantic model requires
a broader correction.
