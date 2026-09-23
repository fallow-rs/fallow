# Development documentation

Start with the smallest route that matches the task:

- [Task context map](task-context-map.md) for ordered reads and explicit skips.
- [Repository map](repo-map.md) for crate and pipeline ownership.
- [Quality gates](quality-gates.md) before validation, review, commit, or push.
- [Release procedure](release-procedure.md) for the maintainer publication steps.
- [Release security](release-security.md) for publication workflow boundaries.
- [Drift contract](drift-contract.md) when a change touches behavior that
  more than one command, the MCP server or the API shares.
- [Review routing](review-routing.md) when a change crosses output or
  integration surfaces.
- [AI tooling](ai-tooling.md) when changing agent instructions, skills, or
  adapters.
- [Agent template](../../.agents/agents/_template.md) when adding a new repository-tracked
  specialist agent.
- [Knowledge architecture](knowledge-architecture.md) when changing
  documentation ownership, synchronization, or publication.

These references are tool-neutral and tracked. Codex and Claude must route to
the same files rather than maintaining separate factual copies.
