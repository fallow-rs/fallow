### Added

- **MCP `analyze` runs as a task when the environment or the config turns on type-aware analysis.** Before, `analyze` returned an MCP task only when a parameter asked for type-aware analysis. Now `FALLOW_TYPE_AWARE` and `typeAware.enabled` in the project config count too, when the parameters leave the choice open. A type-aware run can take about a minute on a large repository, and a synchronous call stops at the 120-second default. `type_aware: false` keeps the call synchronous.
  (Refs [#5237](https://github.com/fallow-rs/fallow/issues/5237))
