### Added

- **The MCP server returns long tool calls as tasks.** The server now advertises the MCP tasks extension (`io.modelcontextprotocol/tasks`). When a `tools/call` request declares the extension in its `_meta` client capabilities, `audit`, `mutation_plan`, `find_similar_code` and type-aware `analyze` return a task handle at once. The client polls `tasks/get` for the status and the result, and `tasks/cancel` stops the run. A task run has a default deadline of 30 minutes instead of 120 seconds. `FALLOW_TIMEOUT_SECS` still sets the deadline. A finished result stays readable for at least one hour. A request without the extension gets the synchronous result as before.
  (Closes [#5237](https://github.com/fallow-rs/fallow/issues/5237))
