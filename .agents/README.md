# Maintainer agent resources

`.agents/` is the client-neutral source of truth for Fallow's repository
workflows and reviewer guidance.

## Skills

Canonical maintainer workflows live at:

```text
.agents/skills/<skill-name>/SKILL.md
```

Codex and other Agent Skills clients discover this tree directly. Claude
adapters under `.claude/skills/` are generated from it:

```bash
npm run generate:agent-adapters
npm run check:agent-adapters
```

Never edit generated Claude skill copies. Do not require `.codex/skills`,
sibling checkouts, or machine-local symlinks for repository workflows.

Every skill must:

- use matching lowercase kebab-case directory and frontmatter names
- include a trigger-oriented description
- keep procedural instructions concise
- route stable architecture and reference knowledge into indexed `docs/`
- avoid private cloud knowledge and local absolute paths

## Runtime-specific configuration

- `AGENTS.md` is the universal repository router.
- `CLAUDE.md` is the thin Claude adapter.
- `.claude/rules/` contains Claude-specific path-scoped constraints.
- `.claude/agents/` and `.codex/agents/` contain runtime-specific reviewer
  definitions where their formats genuinely differ.

Runtime adapters may differ in format, but their observable review contract
must remain equivalent and pass the knowledge architecture gate.

## Public end-user skills

This tree is for developing Fallow itself. The released product skill contract
under `npm/fallow/skills/fallow/` is consumed by the separate public
`fallow-skills` repository, which owns portable plugin packaging and additional
end-user workflows.
