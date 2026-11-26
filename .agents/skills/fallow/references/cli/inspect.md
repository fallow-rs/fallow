# `inspect`: Target Evidence Bundle

Compose one evidence bundle before editing a file or exported symbol. This is the CLI equivalent of the MCP `inspect_target` tool. The MCP tool takes `cloud: true` for `--cloud`, `repo` for `--repo`, and `format: "compact"` for `--format compact`.

## Usage

```bash
fallow inspect --file src/api.ts --format json --quiet
fallow inspect --symbol src/api.ts:fetchUser --format json --quiet
fallow inspect --file src/api.ts --churn --format json --quiet
fallow inspect --file src/api.ts --cloud --format json --quiet
fallow inspect --file src/api.ts --cloud --format compact
```

## Target Flags

| Flag | Description |
|------|-------------|
| `--file <PATH>` | Inspect one project-relative file |
| `--symbol <FILE:EXPORT>` | Inspect one exported symbol. Supporting dead-code, duplication, complexity, and security evidence is file-scoped in the first version |
| `--churn` | Add target-level git churn evidence. Off by default; missing git history is reported as `unavailable` |
| `--cloud` | Add the `cloud` evidence section for a `--file` target: Fallow Cloud production data per local function (state, calls, error groups, CPU ms per hour, never-run branches). Off by default. Key: `$FALLOW_API_KEY`, else the key that `fallow cloud login` stored. The section `status` is `ok`, `no_key`, `plan_required`, `unavailable` or `untracked`; a status other than `ok` never fails the run. Requires fallow 3.38.0 or later |
| `--repo <OWNER/REPO>` | Fallow Cloud repository for `--cloud`. Precedence: this flag > `$FALLOW_REPO` > the `origin` git remote |

`--format compact` for `inspect` requires fallow 3.38.0 or later. It prints plain text of about 1 KB, one line per item and no colors: a `fallow <path>` header, `finding <kind> <path:line> <text>` lines, `prod <name:line> <text>` lines (functions with errors first, then `never_called`, then local functions without production data, then the rest), a `cloud <status>: <text>` line for a status other than `ok`, and `truncated <n> more` when the byte cap cuts lines.

Local functions pair with deployed functions by `stable_id` first, then by the name plus the ordinal among the same-name functions of the file. A local function without a pair goes to `unpaired_local` with the note `no production data (new or moved since deploy)`. A `null` count means "not measured", never zero.

Common global flags: `--format`, `--quiet`, `--root`, `--config`, `--workspace`, `--production`, `--no-cache`, `--threads`.

## JSON Output Structure

```json
{
  "kind": "inspect_target",
  "target": { "type": "file", "file": "src/api.ts" },
  "identity": {
    "file": "src/api.ts",
    "is_reachable": true,
    "is_entry_point": false,
    "export_count": 3,
    "import_count": 2,
    "imported_by_count": 1
  },
  "evidence": {
    "trace_file": { "status": "ok", "scope": "file", "data": {} },
    "dead_code": { "status": "ok", "scope": "file", "data": {} },
    "duplication": { "status": "ok", "scope": "project_filtered_to_file", "data": {} },
    "complexity": { "status": "ok", "scope": "project_filtered_to_file", "data": {} },
    "security": { "status": "ok", "scope": "file", "data": {} },
    "churn": { "status": "ok", "scope": "project_filtered_to_file", "data": {} }
  },
  "warnings": []
}
```

Each evidence section carries `status` and `scope`. The optional churn section can report `ok`, `unavailable`, or `error`. Non-fatal analysis failures become section-level errors and warnings, so callers can still use the remaining evidence.
