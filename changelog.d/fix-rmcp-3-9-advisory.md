### Security

- **The MCP server requires `rmcp` 3.9 or later.** `rmcp` 3.8.0 has the advisory RUSTSEC-2026-0462, unbounded response buffering in the Streamable HTTP clients. The Fallow MCP server uses stdio and does not enable these client transports, so it was not exposed. The update clears the dependency audit.
