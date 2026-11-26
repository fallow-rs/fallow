# `telemetry`: Opt-in Product Telemetry

Manage opt-in, off-by-default product telemetry that helps prioritize agent, CI, MCP, and editor workflows. Fallow never collects repository names, file paths, package or dependency names, source code, config values, environment variable names or values, raw command lines, or raw errors. Hashing those values is not used as a workaround.

```bash
fallow telemetry status              # effective state, source, and config path
fallow telemetry enable              # opt in (user action only; agents must not run this)
fallow telemetry disable             # opt out
fallow telemetry inspect --example   # print an example payload + field purposes
```

Inspect the exact payload a real command would send, without sending it:

```bash
FALLOW_TELEMETRY=inspect fallow audit --format json --quiet
```

The inspected payload prints to stderr; stdout (including `--format json`) is untouched.

## Behavior

- **Off by default.** Precedence: `DO_NOT_TRACK` / `FALLOW_TELEMETRY_DISABLED` (kill switches) > `FALLOW_TELEMETRY_DEBUG` (forces inspect) > `FALLOW_TELEMETRY` env > CI (off unless `FALLOW_TELEMETRY` is set) > user config (`fallow telemetry enable/disable`) > off.
- **CI is off** unless `FALLOW_TELEMETRY` is explicitly set in that CI environment; a local `enable` never turns on org CI telemetry.
- **Decision status:** `fallow telemetry status --format json` includes `explicit_decision`. `false` means the user may have only seen the notice; `true` means `telemetry enable` or `telemetry disable` was explicitly run.
- **Transport:** when enabled, one small JSON event is POSTed to `https://api.fallow.cloud/v1/telemetry/events` (override with `FALLOW_API_URL`), no auth token, no cookies, on a background thread so it does not delay your command. Delivery is best-effort; errors never change output or exit code.
- **Agent source:** wrappers may set `FALLOW_AGENT_SOURCE=<allowlisted-value>` so an enabled run is attributed correctly. Allowlist: `codex`, `claude_code`, `cursor`, `copilot`, `opencode`, `aider`, `roo`, `windsurf`, `gemini` (aliases `gemini_cli`/`antigravity`), `cline`, `continue`, `zed`, `goose`, `other_known`, `unknown`, `none`. Setting it never enables telemetry and uploads no codebase content.
