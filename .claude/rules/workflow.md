# Native Fallow workflow

For non-trivial changes use, in order:

1. `implement`
2. `panel-review` for design decisions
3. `review`
4. `ship`
5. `sweep`

Canonical instructions live under `.agents/skills/`. Claude copies under
`.claude/skills/` are generated adapters and must not be edited.

The active plan belongs under the gitignored `.plans/` directory. Durable
architecture and decisions belong in indexed public documentation. Never put
private cloud knowledge or process scratch files in this public repository.

Read `docs/development/task-context-map.md` before loading detailed references.
