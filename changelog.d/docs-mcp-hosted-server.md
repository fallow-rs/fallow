### Added

- **The `fallow` skill now points agents at the hosted Fallow Cloud MCP server.** `references/mcp.md` gives the endpoint `POST https://api.fallow.cloud/mcp`, the Bearer read-only or live API key, the Claude Code connect command, the explicit arguments that the hosted server needs, and the tools that only the hosted server has: `get_file_context`, `verify_deployment`, `list_errors`, `get_branch_usage`, `get_runtime_dependencies`, `get_deployments` and `get_repo_activation`.
