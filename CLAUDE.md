# Fallow repository adapter for Claude

Fallow is a Rust-native codebase analyzer for JavaScript and TypeScript.

Read `@AGENTS.md` first. It is the universal repository contract for Claude,
Codex, and contributors. Use `@docs/README.md` as the central documentation
index, then use
`@docs/development/task-context-map.md` to load only the references required by
the current task.

## Knowledge layers

- `.agents/skills/` is the canonical, client-neutral maintainer workflow tree.
- `.claude/skills/` is generated from `.agents/skills/`. Never edit it by hand.
- `.claude/rules/` contains only Claude-specific, path-scoped constraints.
- `docs/` contains durable public maintainer architecture and references.
- `.plans/` is gitignored task state. Promote durable decisions into indexed
  public documentation.

## Workflow

Use the native lifecycle for non-trivial work:

1. `implement`
2. `panel-review` when the change has design or user-facing decisions
3. `review`
4. `ship`
5. `sweep`

The controlling workflow lives in the matching `.agents/skills/<name>/SKILL.md`.
Do not layer unrelated workflow systems onto this lifecycle.

## Trust boundary

This public repository must contain only public OSS knowledge and contracts.
Private cloud architecture, operations, customers, commercial context, and
incidents belong in the private cloud repository. Public behavior may be
documented here or in the public user documentation repository, but private
prose must never be copied or automatically exported.

## Generated surfaces

Run `npm run generate:contracts:check` and
`npm run check:agent-adapters` after changing public contracts or skills.
Generated files identify their source and must not be edited directly.
