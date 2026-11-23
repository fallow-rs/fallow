### Fixed

- **Watch reloads when a workspace manifest changes or an editor replaces the selected config file, including a config outside the project.** This includes `pnpm-workspace.yaml`, `deno.json`, and `deno.jsonc`.
- **A scoped file list keeps the file count for each boundary zone in the project.** The positional path still limits the file inventory.
