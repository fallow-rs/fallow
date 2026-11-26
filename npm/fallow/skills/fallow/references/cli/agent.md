# `agent`: One-Pass Agent Onboarding

Wires fallow into the coding-agent harnesses a project uses. `install` detects Claude Code, Codex, and Cursor from the project (`.claude/`, `CLAUDE.md`, `.mcp.json`, `.codex/`, `.cursor/`; `AGENTS.md` is not a signal because every harness and fallow itself write it), the home directory, and the session environment (`CLAUDECODE`, `CODEX_THREAD_ID`, `CURSOR_AGENT`), or takes `--harness`. When nothing is detected only harness-neutral files are written (`AGENTS.md`, `.agents/skills/fallow`, and `.agents/skills/fallow-setup`).

Steps per harness:

| Step | Claude Code | Codex | Cursor |
|---|---|---|---|
| `guide` | `AGENTS.md` task map; `CLAUDE.md` gains an `@AGENTS.md` import (created when absent, appended as a marked block otherwise) | `AGENTS.md` task map | `AGENTS.md` task map (Cursor reads it) |
| `skill` | `.claude/skills/fallow/` and `.claude/skills/fallow-setup/` | `.agents/skills/fallow/` and `.agents/skills/fallow-setup/` | `.agents/skills/fallow/` and `.agents/skills/fallow-setup/` |
| `mcp` | `mcpServers.fallow` in `.mcp.json` (`--approve` also lists it in `.claude/settings.local.json`) | `[mcp_servers.fallow]` in `.codex/config.toml` (applies once the project is trusted; the `codex mcp add` next step works immediately) | `mcpServers.fallow` in `.cursor/mcp.json` |
| `hooks` | `.claude/settings.json` PreToolUse gate plus `.claude/hooks/fallow-gate.sh` | `.codex/hooks.json` PreToolUse gate plus `.codex/hooks/fallow-gate.sh` (Codex runs it after you trust it in `/hooks`), and a marked routing block in `AGENTS.md` | skipped (`unsupported_harness`) |

The step writes each released skill: `fallow` for analysis and `fallow-setup` for setting up code-quality tooling. Each skill is a small pointer to `node_modules/fallow/skills/<name>` when that copy exists (so it never drifts from the installed binary); otherwise the tree embedded in the binary is written. Each skill picks its source on its own, so an older npm package without `fallow-setup` still gets the embedded copy of that skill. The MCP command is probed before anything is written: `npx --no fallow-mcp` for an npm-installed project, `fallow-mcp` from `PATH`, or the running multicall binary. A user-level entry (`--user`) and the printed `codex mcp add` and `claude mcp add --scope user` commands never use `npx`, because a user-level server starts in every project: they use the absolute path of the first `fallow-mcp` on `PATH`, as found, that is outside the project and has no `node_modules` component, or of the running multicall binary under the same rule. `agent status` reports a user-level `npx` entry and a user-level gate script from an older fallow as `stale`. When no launcher exists the step is `skipped` with `mcp_entry_unavailable` rather than writing a config that cannot start.

Every file or block carries a `<!-- fallow:agent-install v1 ... -->` marker. Re-running is byte-stable. An existing skill named `fallow` or `fallow-setup` without a marker is `refused` (`skill_name_taken`) unless `--force`; the other skill is still written. JSON and TOML cannot carry a marker, so a `fallow` MCP entry counts as fallow-managed only when its command is one fallow writes; any other entry is `refused` (`mcp_entry_foreign`) and never removed without `--force`. `--force` on an unparsable config file saves the old bytes as `<file>.fallow-bak` before rewriting. `uninstall` removes managed content, deletes a config file it emptied (and an emptied `.cursor/` or `.codex/` directory), and deletes `AGENTS.md` or `CLAUDE.md` only while the file still matches what fallow authored.

## Flags

| Flag | Applies to | Description |
|---|---|---|
| `--harness <auto\|claude\|codex\|cursor>` | `install`, `uninstall` | Repeatable; default `auto` |
| `--without <guide\|skill\|mcp\|hooks>` | `install` | Skip a step; repeatable |
| `--dry-run` | `install`, `uninstall` | Print the plan without touching the filesystem |
| `--force` | `install`, `uninstall` | Replace or remove skills, hook scripts, or config files fallow did not write |
| `--approve` | `install` | Pre-approve the project MCP server for yourself in `.claude/settings.local.json`; refused when that file is tracked by git |
| `--user` | `install`, `uninstall` | Skill, MCP config, and gate under `$HOME` (`~/.claude/skills`, `~/.agents/skills`, `~/.codex/config.toml`, `~/.cursor/mcp.json`, `~/.claude/hooks`, `~/.codex/hooks.json`); the guide step and the `AGENTS.md` routing block are skipped, and Claude Code prints the `claude mcp add --scope user` command instead of editing `~/.claude.json` |
| `--gitignore-claude` | `install` | Append `.claude/` to `.gitignore` |

Root: the git toplevel of the current directory unless `--root` is passed explicitly, so a run from a monorepo package still writes where the harnesses read. The chosen root is the first line of output and `root` in JSON.

## Output

Human output groups paths under "Shared with your team (commit these)" and "Local to you". JSON:

```json
{
  "kind": "agent-install",
  "schema_version": 1,
  "fallow_version": "3.49.0",
  "root": "/abs/path",
  "mode": "install",
  "dry_run": false,
  "harnesses": ["claude", "codex"],
  "detected": true,
  "evidence": [{"harness": "claude", "evidence": [".claude", "$CLAUDECODE"]}],
  "steps": [
    {"harness": null, "step": "guide", "status": "written", "scope": "shared", "path": "AGENTS.md"},
    {"harness": "claude", "step": "mcp", "status": "skipped", "scope": "local", "path": ".claude/settings.local.json", "reason": "approval_not_requested"}
  ],
  "next_actions": [{"id": "codex-mcp-add", "command": "codex mcp add fallow -- /usr/local/bin/fallow-mcp", "reason": "...", "mutating": true}]
}
```

`status` values: `written`, `removed`, `unchanged`, `skipped`, `refused`, `failed`. Exit code 2 when any step is `refused` or `failed`; every other step still runs. `kind` is `agent-install`, `agent-uninstall`, or `agent-status`; `schema_version` is `1`; `agent status` reports `surfaces[]` with `installed`, `stale`, `absent`, or `foreign`. `next_actions` is deliberately not `next_steps`: entries flagged `mutating: true` write harness config when run, unlike the read-only `next_steps[]` of the analysis commands.

## Examples

```bash
fallow agent install --dry-run           # show the plan for the detected harnesses
fallow agent install                     # wire everything detected
fallow agent install --harness claude --approve
fallow agent install --without hooks --format json --quiet
fallow agent status --format json
fallow agent uninstall --dry-run
```
