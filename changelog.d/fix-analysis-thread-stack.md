### Fixed

- **The language server and the MCP `decision_surface` tool no longer abort on deeply nested source.** Both ran analysis on threads with a 2 MiB stack. A deeply nested file overflowed that stack and stopped the process. The language server now gives its analysis threads the 16 MiB stack of the CLI workers. `decision_surface` and `run_decision_surface` now parse on the analysis worker pool, as the other tools do. Their thread count now follows the `threads` option.
