---
name: conformance-loop
description: Iteratively improve Fallow analysis accuracy by comparing it with competing tools and verified source truth across a stable real-world corpus.
---

# Conformance loop

1. Define the capability and stable project corpus.
2. Run Fallow and comparison tools with documented equivalent settings.
3. Manually verify disagreements against source.
4. Classify true positive, false positive, false negative, or model difference.
5. Implement one general correction with a regression fixture.
6. Re-run the full corpus and retain only net improvements.
7. Run `review`.

Competitor output is a lead, not ground truth.
