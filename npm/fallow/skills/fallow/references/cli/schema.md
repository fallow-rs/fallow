# `schema`: Capability Manifest

Dumps fallow's complete capability manifest as machine-readable JSON (always JSON, regardless of `--format`). The single source of truth for agent introspection.

```bash
fallow schema
```

Top-level blocks:

- `manifest_version`: manifest shape discriminator (currently `"1"`).
- `commands` + `global_flags`: every CLI command and flag, derived live from the CLI definition.
- `issue_types`: one row per reportable issue type across ALL analyses (dead-code, health, dupes, flags, security). Each row carries `id` (the bare rule id; several rows share one suppression token, e.g. all complexity rules suppress via `complexity`), `rule_id` (SARIF id), `command`, `category`, `filter_flag` (null when none), `fixable`, `suppressible`, `suppress_comment` (copy-pasteable, null when not suppressible), `note`, `license` (`free` | `freemium`), and `docs_url`. Nullable fields are always present (null, never absent).
- `mcp_tools`: all MCP server tools with `kind` grouping (analysis/trace/impact/fix/introspection/runtime-coverage/composition), one-line description, `cli_command` nearest CLI fallback, `key_params` (curated subset; live MCP `list_tools` schemas are authoritative), `license` + `license_note` (the 5 runtime-coverage tools are `freemium`: a single local capture is free, continuous monitoring is paid), `read_only` (`false` also when the tool writes only a report or baseline file on request), and `modifies_source` (`true` only when the tool edits source files, such as `fix_apply`).
- `plugins`: built-in framework plugin count + names, derived live from the registry.
- `environment_variables`: every user-facing `FALLOW_*` variable (internal plumbing excluded).
- `output_formats`, `exit_codes`, `severity_levels`, `suppression_comments`.
