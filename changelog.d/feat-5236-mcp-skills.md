### Added

- **The MCP server serves the fallow skills.** `fallow-mcp` now supports the MCP skills extension (`io.modelcontextprotocol/skills`). A client that supports the extension, such as Claude Code 2.1.331 or later, can load the `fallow` and `fallow-setup` skills from the server, at the same version as the tools. Each skill is at `skill://<name>/SKILL.md`, and its manifest lists every file with its SHA-256 digest and byte size.
  (Closes [#5236](https://github.com/fallow-rs/fallow/issues/5236))
