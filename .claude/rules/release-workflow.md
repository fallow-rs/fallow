---
paths:
  - ".github/workflows/release.yml"
  - ".github/workflows/scorecard.yml"
  - ".github/zizmor.yml"
---

# Release workflow security boundary

- Read `docs/development/release-security.md` before changing the workflow.
- Preserve the separation between preparation jobs and credential-bearing
  publication jobs.
- Keep Git tag creation and post-publication pin updates in the maintainer
  release workflow.
- Update the shared release-security reference when an invariant changes.
