### Fixed

- **`fallow audit` no longer reports an existing finding on a workspace package import as introduced.** The base worktree shares the `node_modules` of the checkout under review, so its workspace links point at the packages of that checkout, outside the base. The base analysis classified an import such as `@acme/lib` as an npm package and lost the edge, and a boundary violation or another finding on that edge looked introduced in the head. The import now resolves to the workspace package of the base.
