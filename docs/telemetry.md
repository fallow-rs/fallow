# Telemetry

Fallow's product telemetry is opt-in and off by default. It exists to improve agent, CI, JSON, MCP, and editor workflows.

Fallow does not collect repository names, file paths, package names, dependency names, workspace names, source code, config values, environment variable names or values, raw command lines, raw errors, or stack traces.

To keep telemetry off everywhere, set `FALLOW_TELEMETRY_DISABLED=1` or `DO_NOT_TRACK=1`.

## Commands

```bash
fallow telemetry status
fallow telemetry enable
fallow telemetry disable
fallow telemetry inspect --example
```

Inspect the exact payload for a real command without sending it:

```bash
FALLOW_TELEMETRY=inspect fallow audit --format json --quiet
```

`FALLOW_TELEMETRY_DEBUG=1` forces inspect mode and outranks `FALLOW_TELEMETRY`.

## Environment Controls

Precedence:

```text
DO_NOT_TRACK / FALLOW_TELEMETRY_DISABLED   (admin/fleet kill switch)
> FALLOW_TELEMETRY_DEBUG                     (forces inspect mode)
> FALLOW_TELEMETRY                           (per-shell override)
> CI: off unless FALLOW_TELEMETRY is set
> user telemetry config
> default: off
```

Disable telemetry globally in CI or managed environments:

```bash
export FALLOW_TELEMETRY_DISABLED=1
```

Or use the conventional disable flag:

```bash
export DO_NOT_TRACK=1
```

Enable explicitly in CI:

```bash
export FALLOW_TELEMETRY=on
```

CI telemetry is off unless it is explicitly enabled in that CI environment. A developer's local `fallow telemetry enable` does not silently enable organization CI telemetry.

Agents and wrappers can identify their integration with one normalized allowlisted value:

```bash
export FALLOW_AGENT_SOURCE=codex
```

Accepted values are `codex`, `claude_code`, `cursor`, `copilot`, `opencode`, `aider`, `roo`, `windsurf`, `gemini`, `cline`, `continue`, `zed`, `goose`, `other_known`, `unknown`, and `none`. Hyphen aliases such as `claude-code` and CLI aliases such as `gemini_cli` / `antigravity` (both map to `gemini`) are normalized. Unrecognized values are ignored rather than uploaded.

## What Is Collected

V1 events are workflow-level and coarse:

```json
{
  "schema_version": 1,
  "event": "workflow_completed",
  "fallow_version": "2.85.0",
  "workflow": "audit",
  "integration_surface": "mcp",
  "invocation_context": "agent",
  "agent_source": "codex",
  "output_format": "json",
  "quiet": true,
  "ci": false,
  "tty": false,
  "os": "linux",
  "arch": "x86_64",
  "duration_bucket_ms": "500-2000",
  "outcome": "issues_found",
  "exit_code_bucket": "1",
  "findings_present": true,
  "mcp_tool": "find_dupes",
  "parent_run": "tmp_8x7p4k"
}
```

`agent_source`, `failure_reason`, `findings_present`, `mcp_tool`, and `parent_run` are optional. `agent_source` appears only on agent-driven runs. `failure_reason` appears only on failed workflow events and uses one of `validation`, `unsupported_format`, `config`, `analysis`, `diff`, `network`, `auth`, `gate`, `signal`, or `unknown`. `findings_present` is omitted by commands that run no analysis (and by older binaries). `mcp_tool` appears only when a run came through the MCP server. `parent_run` appears only when a run is explicitly correlated to a previous one. All are omitted otherwise.

Field purposes:

| Field | Purpose |
| --- | --- |
| `workflow` | Prioritize the audit, dead-code, health, duplication, CI, runtime-coverage setup, impact, security, fix, explain, project-inventory, setup, and license workflows. Project-inventory, setup, and license are coarse buckets and do not expose raw commands, config values, repository identifiers, or license identifiers. |
| `integration_surface` | Understand whether Fallow is used through human CLI, CLI JSON, MCP, CI, editor, or programmatic surfaces. |
| `invocation_context` | Separate human, CI, editor, and agent-driven use without uploading detection evidence. |
| `agent_source` | Improve compatibility with specific agent integrations using a documented allowlist. |
| `output_format` / `quiet` | Protect the output contracts that users and agents rely on most. |
| `duration_bucket_ms` | Find slow workflow classes without collecting exact timings. |
| `outcome` / `exit_code_bucket` | Measure clean runs, findings, and failures without uploading raw error text. |
| `failure_reason` | Group failed workflows by a fixed privacy-safe allowlist; unknown stays `unknown` instead of parsing raw error text. |
| `findings_present` | Whether the analysis surfaced any findings, decoupled from the exit-code gate (so informational analyses like default-config `dupes`, which never exit non-zero, are still measurable). On the combined and audit workflows it is an OR across the sub-analyses; per-analysis find-rate is answerable on the standalone `dead_code`, `dupes`, `health`, and `security` workflows. |
| `mcp_tool` | Attribute MCP usage to a specific tool, from a fixed allowlist of tool names. |
| `parent_run` | Link explicit agent follow-up runs using a short allowlisted token, never a path or free-form string. |

## Integration surfaces

The MCP server runs Fallow by invoking the CLI, so an MCP tool call already produces one CLI telemetry event. The server tags that spawned process (via the `FALLOW_INTEGRATION_SURFACE` and `FALLOW_MCP_TOOL` environment variables) so the single event is attributed to the `mcp` surface and the specific tool, instead of looking like any other `cli_json` run. No second event is emitted, and the privacy posture is identical because it is the same CLI code path and consent check. `FALLOW_MCP_TOOL` is validated CLI-side against a fixed allowlist of tool names; any other value is dropped.

The LSP server, VS Code extension, N-API bindings, and programmatic embedding run analysis in-process rather than by spawning the CLI, so the environment-variable tagging does not reach them and they emit no telemetry today. Their `integration_surface` values are reserved for when a future shared telemetry layer lets them emit directly.

## Agent Source

When telemetry is enabled and a run is classified as agent-driven, Fallow may emit one normalized `agent_source` value:

```text
none
codex
claude_code
cursor
copilot
opencode
aider
roo
windsurf
gemini
cline
continue
zed
goose
other_known
unknown
```

`none` appears in the list because it is the internal default before a run is classified, but it is never sent: `agent_source` is only emitted for runs Fallow identifies as agent-driven, and those are never `none`. Agents not on the list (for example enterprise IDE assistants) are grouped under `other_known`.

Fallow does not upload raw MCP client info, process names, parent process paths, editor identifiers, extension names, environment variable names, model names, account IDs, organization IDs, prompts, versions, or free-form vendor strings. Agent wrappers should use `FALLOW_AGENT_SOURCE=<allowlisted-value>` when the user has enabled telemetry; ambiguous sources emit `unknown`. **Setting `FALLOW_AGENT_SOURCE` never enables telemetry by itself and uploads no codebase content.**

When several agent environments coexist (for example one agent running inside another), heuristic `agent_source` attribution is best-effort and depends on environment iteration order. Set `FALLOW_AGENT_SOURCE` explicitly for deterministic attribution.

## What Is Never Collected

Fallow telemetry must not include:

- repository, organization, project, branch, or git remote names
- file paths, import specifiers, source snippets, or stack traces
- package, dependency, workspace, or framework package names
- raw command-line arguments
- config contents or config values
- environment variable names or values
- raw errors, logs, or serialized exceptions
- stable machine, user, project, or repository identifiers

Hashing these values is not used as a workaround.

## Agent Follow-up

The `--parent-run` flag and the `parent_run` field ship today (the flag is hidden from `--help` until the rest of this mechanism lands). `--parent-run` accepts only short ASCII tokens with letters, numbers, `_`, and `-`; paths and free-form values are dropped before upload. The piece that is still future is the server-issued `_meta.telemetry.analysis_run_id` that an agent would pass back via `--parent-run` so Fallow can measure whether a follow-up run improved aggregate findings.

Agents must not enable telemetry automatically. `fallow telemetry enable` requires explicit user action in a human-controlled shell or explicit CI environment configuration.

## Transport And Server Privacy

When telemetry is enabled and sending events:

- requests are HTTPS POST JSON to `https://api.fallow.cloud/v1/telemetry/events` (override the host with `FALLOW_API_URL`)
- no cookies are used
- telemetry requests do not carry an authentication token
- the upload runs on a background thread, so it does not slow down your command
- Fallow does not wait for the upload, so the fastest runs and runs on slow networks often drop their event; counts are a rough, biased sample, not an exact usage count
- network errors are ignored and never affect command output or exit code
- telemetry is never written to stdout
- server-side handling must not enrich telemetry with customer, repository, organization, git, package-registry, or license data
- IP addresses are dropped or truncated as early as practical
- raw events are retained only for a short documented window, then deleted

Public reporting uses only coarse aggregate trends after privacy review.
