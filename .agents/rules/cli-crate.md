---
paths:
  - "crates/cli/**"
---

# CLI routing

Follow [`crates/cli/AGENTS.md`](../../crates/cli/AGENTS.md) for ownership and
validation, then [`docs/reference/cli-internals.md`](../../docs/reference/cli-internals.md)
for command orchestration, coverage resolution, mutation, and cache invariants.

For public contracts, use [`docs/backwards-compatibility.md`](../../docs/backwards-compatibility.md),
[`docs/output-schema.json`](../../docs/output-schema.json), and the live CLI help.
Environment behavior lives in [`docs/environment-variables.md`](../../docs/environment-variables.md).
Security findings remain candidates; consult
[`docs/security-agent-verification.md`](../../docs/security-agent-verification.md)
before changing their interpretation or presentation.

Use the applicable output review skill for human, JSON, or CI format changes.
Keep shared analysis and serialized report construction in their engine, API,
and output owners instead of duplicating them in command adapters.
