# `init`: Config Generation

Creates a config file in the project root.

## Flags

<!-- generated:flags:init:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--toml` | `bool` | `false` | Create `fallow.toml` instead of `.fallowrc.json` |
| `--agents` | `bool` | `false` | Scaffold a starter `AGENTS.md` guide for coding agents. Prefills Install (from the `packageManager` field, or pnpm via `pnpm-workspace.yaml`), Test (only when exactly one of Vitest / Jest / Playwright is present), Typecheck (`tsc --noEmit` when `tsconfig.json` exists), and monorepo module-boundary lines; everything ambiguous stays blank (no lockfile sniffing). Prefilled command lines carry an HTML provenance comment. Refuses to overwrite an existing `AGENTS.md` |
| `--hooks` | `bool` | `false` | Scaffold a pre-commit git hook that runs `fallow audit --staged --quiet --gate-marker pre-commit`. Alias for `fallow hooks install --target git` |
| `--branch` | `string` | - | Has no effect. The pre-commit hook audits the staged changes against HEAD. Kept so that existing install scripts still run. Only used with `--hooks` |
| `--decline` | `bool` | `false` | Record that this project deliberately stays unconfigured: persists a decline so the first-contact setup hint and the `setup` next-step stop appearing here. Writes no config file; idempotent |

Common global flags for this command: [`--root`](global-flags.md), [`--config`](global-flags.md).
<!-- generated:flags:init:end -->
## Examples

```bash
fallow init              # creates .fallowrc.json with $schema
fallow init --toml       # creates fallow.toml
fallow init --agents     # scaffolds a starter AGENTS.md prefilled from detected project info (never overwrites)
fallow hooks install --target git   # pre-commit hook: fallow audit --staged
```
