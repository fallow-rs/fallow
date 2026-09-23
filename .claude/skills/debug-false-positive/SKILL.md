---
name: debug-false-positive
description: Diagnose and fix a Fallow false positive or false negative through extraction, resolution, graph, analysis, reporting, and real-consumer verification.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Debug a false result

1. Reproduce on current `main` with `--format json --quiet`. Save one command
   that shows the wrong result. Change no code before this command exists.
   After a restart or an update, clear the `.fallow/` cache first.
2. Reduce to a minimal fixture without losing the behavior.
3. Write 3 to 5 ranked hypotheses. Each one predicts a result that one probe
   can disprove. Test one hypothesis per probe.
4. Trace the finding through extract, resolve, graph, reachability, analysis,
   suppression, filters, and report assembly.
5. Fix the earliest incorrect layer. Fix the pattern, not the instance (see
   test evidence in `docs/development/quality-gates.md`). After 3 failed
   fixes, stop and write down the premise that they share. Question that
   premise before a fourth fix.
6. Add a regression test that fails without the fix.
7. Run the fixed binary on a real consumer project with a cleared `.fallow/`
   cache and compare old versus new output.
8. Run `review` with the affected surface reviewers.

Do not tune expected output around one fixture when the semantic model requires
a broader correction.
