### Fixed

- **A flow-style catalog group is no longer reported as empty.** A group such
  as `ui: { react: "18" }` in `pnpm-workspace.yaml` gave an
  `empty-catalog-group` finding, and `fallow fix` deleted the line. Its
  entries now resolve `catalog:ui` references. A catalog group that is a YAML
  alias of another group is also no longer reported as empty.
- **A flow-style `packages` list in `pnpm-workspace.yaml` declares
  workspaces.** `packages: [packages/*, apps/*]` gave no workspace. A trailing
  comment on a `packages` entry is no longer part of the pattern. `fallow
  init` reads the same list.
- **Flow-style `overrides` in `pnpm-workspace.yaml` are read.** Their
  `catalog:` values now count as catalog consumers, and the override checks
  see them.
