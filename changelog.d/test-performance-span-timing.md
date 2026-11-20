### Fixed

- **`fallow dead-code --performance` now puts the config load and the workspace discovery in the `config` span.** The config load ran before the `config` span started, so `config_ms` showed almost no time and the load counted as unmeasured time. The span tree also put `workspaces` under `analysis`, but the config load discovers the workspaces before the analysis starts. `workspaces` is now a child of `config`, and the other stages always fit inside the `analysis` span.
