### Added

- **`fallow security` says when `client-server-leak` checked nothing.** The rule starts only from files with a top-level `"use client"` directive. When no such file exists, for example in a React Router Framework Mode app or a Vite SPA, the run now records the `client-server-leak-no-client-roots` workspace diagnostic. The human output prints a blind-spot line, and MCP adds a `warnings` sentence. Other commands record the diagnostic only when the config sets the rule to `warn` or `error`.
  (Closes [#4430](https://github.com/fallow-rs/fallow/issues/4430))
