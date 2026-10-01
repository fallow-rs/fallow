# Configure and install

## Recommendation

`fallow recommend --format json --quiet` inspects the project and changes nothing. It always exits 0. The envelope has `kind: "recommendation"` and these fields:

| Field | Content |
|---|---|
| `detected` | The project facts: monorepo layout, TypeScript, test framework, UI framework, Storybook, package manager |
| `proposed_config` | A safe starting config |
| `decisions[]` | One entry per setting: `setting`, `value`, `rationale`, `kind`, `question` |
| `config_schema_command` | The command that prints the config JSON Schema |

Use `kind` to decide what to do with each decision:

| `kind` | Meaning | Action |
|---|---|---|
| `auto` | Fallow decided the value from the detection | Apply it. |
| `default` | A disclosed default that the user can change | Apply it and tell the user the `rationale` in one line. |
| `taste` | A subjective choice | Keep the current value, or ask the user. `question` has a `header`, a `question`, and `options[]` with a `label` and a `description`. Show these options as they are. |

When the project has no Fallow config, write `proposed_config` plus the answers to `.fallowrc.json`. When the project has a config, change only the settings that the user approved. Validate the keys with `fallow config-schema`. Check the loaded config with `fallow config --format json --quiet`.

When the project has a Knip, jscpd, or stylelint config, preview the migration with `fallow migrate --dry-run`. Merge the result with the recommendation. Do not delete the old config in this step. The [parity check](tooling-detection.md#parity-check) decides when it can go.

## Dev dependency

Install Fallow as a dev dependency with the detected package manager:

| Package manager | Command |
|---|---|
| npm | `npm install --save-dev fallow` |
| pnpm | `pnpm add -D fallow` |
| Yarn | `yarn add -D fallow` |
| Bun | `bun add -d fallow` |

A dev dependency pins one Fallow version for every developer, every agent, and CI. Add scripts to `package.json` when the project uses scripts for its other checks, for example `"fallow": "fallow"` and `"fallow:audit": "fallow audit"`.

## Agent wiring

`fallow agent install` wires Fallow into Claude Code, Codex, and Cursor in one pass. It detects the harnesses from the project, the home directory, and the session environment. `--harness` selects them explicitly.

1. Run `fallow agent install --dry-run --format json --quiet`. Each entry in `steps[]` has a `step`, a `status`, and a `path`. Show the plan to the user.
2. Run `fallow agent install`. Add `--without <step>` to skip a step. The steps are `guide`, `skill`, `mcp`, and `hooks`.
3. Run `fallow agent status --format json --quiet`. Check only the entries of the harnesses that step 1 selected. Status also lists the other harnesses (for example `.cursor/mcp.json` in a Claude-only project) as `absent`, which is correct. Act on `next_actions[]` for a `stale` entry of a selected harness.

The steps write these items:

| Step | Result |
|---|---|
| `guide` | The task map in `AGENTS.md`, and an `@AGENTS.md` import in `CLAUDE.md` for Claude Code |
| `skill` | The Fallow skills under `.claude/skills/` and `.agents/skills/` |
| `mcp` | The MCP server registration for each harness |
| `hooks` | The commit and push gate: a PreToolUse hook for Claude Code (`.claude/settings.json`) and for Codex (`.codex/hooks.json`), plus a routing block in `AGENTS.md` |

Exit code 2 means that a step is `refused` or `failed`. Read the `reason` of that step. `skill_name_taken` means that a skill with the same name exists and Fallow did not write it. Do not pass `--force` without the approval of the user.
