### Fixed

- **`new URL("./lib/sign.ts", import.meta.url)` now credits the `default` export of the module.** A build tool or a script runner loads such a module by path and calls its default export. Before, the analysis credited the file but reported its `default` export as unused. Other exports stay reportable. A module that a `Worker` or `SharedWorker` constructor gets, a directory URL, and a loader hook module of `module.register()` keep their earlier credit. The cached extraction facts change, so `CACHE_VERSION` goes to 409 and `GRAPH_CACHE_VERSION` goes to 102.
