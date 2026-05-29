# Telemetry

Fallow telemetry is opt-in product telemetry for improving agent, CI, JSON, MCP, and editor workflows.

Telemetry is off by default. Fallow does not collect repository names, file paths, package names, dependency names, workspace names, source code, config values, environment variables, raw command lines, raw errors, or stack traces.

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

`FALLOW_TELEMETRY_DEBUG=1` is an alias for inspect mode.

## Environment Controls

Precedence:

```text
DO_NOT_TRACK / FALLOW_TELEMETRY_DISABLED
> FALLOW_TELEMETRY
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
  "fallow_version": "2.84.0",
  "workflow": "audit",
  "integration_surface": "cli_json",
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
  "exit_code_bucket": "1"
}
```

Field purposes:

| Field | Purpose |
| --- | --- |
| `workflow` | Prioritize audit, dead-code, health, duplication, CI, and runtime-coverage setup workflows. |
| `integration_surface` | Understand whether Fallow is used through human CLI, CLI JSON, MCP, CI, editor, or programmatic surfaces. |
| `invocation_context` | Separate human, CI, editor, and agent-driven use without uploading detection evidence. |
| `agent_source` | Improve compatibility with specific agent integrations using a documented allowlist. |
| `output_format` / `quiet` | Protect the output contracts that users and agents rely on most. |
| `duration_bucket_ms` | Find slow workflow classes without collecting exact timings. |
| `outcome` / `exit_code_bucket` | Measure clean runs, findings, and failures without uploading raw error text. |
| `parent_run` | Link explicit agent follow-up runs using a short allowlisted token, never a path or free-form string. |

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

`none` is an internal classifier state, not a wire value: `agent_source` is only emitted when a run is classified as agent-driven, which by definition is not `none`. Agents not on the list (for example enterprise IDE assistants) are grouped under `other_known`.

Fallow does not upload raw MCP `clientInfo`, process names, parent process paths, editor identifiers, extension names, environment variable names, model names, account IDs, organization IDs, prompts, versions, or free-form vendor strings. Agent wrappers should use `FALLOW_AGENT_SOURCE=<allowlisted-value>` when the user has enabled telemetry. Ambiguous sources emit `unknown`. Low-volume public aggregates are grouped under `other_known`.

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

Future agent-correlation work may include a short-lived `_meta.telemetry.analysis_run_id` in inspect/enabled/explicit-agent contexts. Agents may pass that value back with `--parent-run` so Fallow can measure whether a follow-up run improved aggregate findings. `--parent-run` accepts only short ASCII tokens with letters, numbers, `_`, and `-`; paths and free-form values are dropped before upload.

Agents must not enable telemetry automatically. `fallow telemetry enable` requires explicit user action in a human-controlled shell or explicit CI environment configuration.

## Transport And Server Privacy

When upload support is enabled:

- requests are HTTPS POST JSON
- no cookies are used
- telemetry requests do not carry an authentication token
- the upload runs on a detached thread and never blocks command exit for meaningful time; delivery is best-effort and lossy by design. Because the process does not wait for the upload, the fastest commands and runs on slow networks disproportionately fail to deliver, so event counts are a biased sample, not a usage census
- network errors are ignored and never affect command output or exit code
- telemetry is never written to stdout
- server-side handling must not enrich telemetry with customer, repository, organization, git, package-registry, or license data
- IP addresses are dropped or truncated as early as practical
- raw events are retained only for a short documented window, then aggregated and deleted

Public reporting uses only coarse aggregate trends after privacy review.
