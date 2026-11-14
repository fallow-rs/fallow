### Fixed

- **`includeEntryExports` no longer reports the React Router and Remix `entry.server` exports as unused.** The server runtime reads `default`, `handleDataRequest`, `handleError` and `streamTimeout` from `entry.server`, so these exports now count as used. For React Router, `instrumentations` and `unstable_instrumentations` also count as used. A custom `appDirectory` gets the same rule. Other exports of `entry.server` stay reported.
  (Closes [#4583](https://github.com/fallow-rs/fallow/issues/4583))
