### Fixed

- **Fallow Impact no longer records a run on a project in a system temp folder.** A temp folder holds test fixtures and throwaway clones. Each of these added a store to the user config folder, and a local test suite added thousands of them. This made `fallow impact --all` and the MCP tool `impact_all` slow. Projects outside a temp folder record as before.
  (Closes [#4555](https://github.com/fallow-rs/fallow/issues/4555))
