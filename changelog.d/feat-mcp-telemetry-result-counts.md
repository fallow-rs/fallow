### Added

- **Opt-in telemetry of in-process MCP analysis calls now tells whether the analysis found something.** Events of `analyze`, `check_changed`, `check_architecture`, `find_dupes`, `check_health` and `audit` that run in the MCP server process now carry `findings_present` and the coarse `result_count_bucket`, like the events of the same commands on the CLI. The server reads only the count from the result, and only when telemetry may be on. See [`docs/telemetry.md`](docs/telemetry.md#integration-surfaces).
