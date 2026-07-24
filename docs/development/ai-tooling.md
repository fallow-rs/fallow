# AI tooling

Fallow supports multiple agent hosts through one canonical knowledge model.

## Canonical layers

- `AGENTS.md` is the compact Codex router.
- `CLAUDE.md` is the compact Claude router.
- `docs/` contains durable, host-neutral knowledge.
- `.agents/skills/<name>/SKILL.md` contains the authored maintainer workflow.
- `.claude/skills/<name>/SKILL.md` is a generated Claude adapter.
- `.claude/rules/` contains curated, short Claude constraints and routes.
- `docs/reference/` contains durable implementation detail extracted from
  runtime rules.
- Nested `AGENTS.md` files add subsystem-specific instructions.

Codex reads `.agents/skills` directly. There is no `.codex/skills` source and
no sibling-checkout dependency.

## Adapter contract

Run:

```bash
npm run generate:agent-adapters
npm run check:agent-adapters
```

The generator owns the Claude adapter bytes and marks them as generated.
Hand-edit the canonical `.agents/skills` source, regenerate, then commit both
surfaces. CI runs check mode and rejects drift.

Do not hand-maintain equivalent Claude and Codex workflow prose. Host-specific
frontmatter or discovery metadata belongs in the generator.

## Fresh-clone contract

A clean checkout must provide:

- both root routers,
- every routed durable reference,
- canonical skills,
- generated adapters,
- non-mutating validation commands.

The repository validator checks that routes and indexed documents exist and are
tracked. Agent discovery must never depend on ignored paths, local symlinks,
private mounts, or another checkout.

## Context discipline

Start with [the task context map](task-context-map.md). Read only the references
and skill relevant to the current task. Do not load large catalogues into every
session.

When stable knowledge emerges from a workflow, promote it into `docs/` and link
it from a task route. Keep incident detail and temporary plans out of the
always-loaded routers.
