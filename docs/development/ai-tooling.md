# AI tooling

Fallow supports multiple agent hosts through one canonical knowledge model.

## Canonical layers

- `AGENTS.md` is the compact Codex router.
- `CLAUDE.md` is the compact Claude router.
- `docs/` contains durable, host-neutral knowledge.
- `.agents/skills/<name>/SKILL.md` contains the authored maintainer workflow.
- `.claude/skills/<name>/SKILL.md` is a generated Claude adapter.
- `.agents/agents/<name>.md` contains the authored reviewer agent definitions.
- `.claude/agents/<name>.md` is a generated Claude adapter.
- `.claude/rules/` contains curated, short Claude constraints and routes.
- `docs/reference/` contains durable implementation detail extracted from
  runtime rules.
- Nested `AGENTS.md` files add subsystem-specific instructions.

Codex reads `.agents/skills` and `.agents/agents` directly. There is no
`.codex/skills` source and no sibling-checkout dependency.

## Adapter contract

Run:

```bash
npm run generate:agent-adapters
npm run check:agent-adapters
```

The generator owns the Claude adapter bytes and marks them as generated.
Hand-edit the canonical `.agents/skills` or `.agents/agents` source, regenerate,
then commit both surfaces. CI runs check mode and rejects drift.

Do not hand-maintain equivalent Claude and Codex workflow prose. Host-specific
frontmatter or discovery metadata belongs in the generator.

## Curated agent-doc cells

`scripts/generate-agent-docs.mjs` regenerates the tables in the published
fallow skill tree from the `fallow schema` capability manifest. Identity columns
regenerate on every run. Curated columns (command `Purpose` and `Key Flags`,
the `Description` on issue types, MCP tools, MCP resources, and CLI flags, and
the dead-code filter `Issue Type`) are hand-owned: an existing cell is preserved
verbatim and only a new row is seeded from the manifest.

Preserving prose forever hides one failure mode. The manifest text a cell was
written from can move later, leaving a published cell that describes a surface
that has changed. `scripts/agent-doc-curated-seeds.json` records the seed each
curated cell was last accepted against. It lives beside the generator, outside
the vendored skill tree, so the gate never touches the public skills surface.

- `npm run generate:contracts:check` fails when a recorded seed no longer
  matches. It names the cell as `section / row / column` and prints both the
  recorded and the current seed.
- `npm run generate:contracts` re-records the seeds, so accepting a moved seed
  is one command.

A drifted seed is a prompt to review, not an automatic rewrite. Read the
published cell, correct the prose when it no longer holds, then re-record.

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
