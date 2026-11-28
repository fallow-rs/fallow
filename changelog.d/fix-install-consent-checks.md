### Fixed

- **`fallow agent install --approve` approves the project MCP server only when `.mcp.json` holds the fallow entry.** When `.mcp.json` already had a `fallow` entry that fallow did not write, the install refused to replace it but still listed `fallow` in `enabledMcpjsonServers`. That approved the existing entry and its command. Now the approval row is `skipped` with the reason of the `.mcp.json` row, and `.claude/settings.local.json` stays as it was.
