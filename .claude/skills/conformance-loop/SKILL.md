---
name: conformance-loop
description: Iteratively improve Fallow analysis accuracy by comparing it with competing tools and verified source truth across a stable real-world corpus.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Conformance loop

1. Define the capability and stable project corpus.
2. Run Fallow and comparison tools with documented equivalent settings. Run
   each comparison tool as the project's CI or `package.json` script runs it,
   with the same flags and preprocessors. A bare run can report a very
   different result set.
3. Manually verify disagreements against source.
4. Classify true positive, false positive, false negative, or model difference.
5. Record the classification in `tests/conformance/adjudications.json` before
   writing the fix. A verdict that lives only in a session or in `.plans/` is
   lost, and the daily lane then re-reports the same disagreement as
   unadjudicated forever.
6. Implement one general correction with a regression fixture.
7. Re-run the full corpus and retain only net improvements.
8. Run `review`.

Competitor output is a lead, not ground truth.
