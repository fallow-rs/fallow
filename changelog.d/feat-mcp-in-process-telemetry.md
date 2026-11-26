### Added

- **Opt-in telemetry now counts MCP tool calls that run in the server process.** Most MCP analysis tools run in the server through `fallow-api` and gave no event before, so agent use through MCP was mostly invisible even with telemetry on. After such a call, the server now runs the hidden `fallow telemetry record` command, which applies the usual consent rules and records one coarse event with `integration_surface: mcp` and the tool name. A call that runs the CLI still gets its events from the CLI. The server does this work after the answer, so telemetry never delays a tool call, and with telemetry off it starts no `record` process. See [`docs/telemetry.md`](docs/telemetry.md#integration-surfaces).

### Fixed

- **`DO_NOT_TRACK` and `FALLOW_TELEMETRY_DISABLED` accept any value as an opt-out.** Before, only `1`, `true`, `yes` and `on` disabled telemetry, so `DO_NOT_TRACK=enabled` or `DO_NOT_TRACK=y` left it on. Now every value except empty, `0`, `false`, `no` and `off` disables telemetry and the update check.
