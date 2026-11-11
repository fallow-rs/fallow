### Fixed

- **`fallow dead-code` credits the shared config packages that `extends` in `biome.json` or `biome.jsonc` names.** Before this change, a package such as `@acme/biome-config` in `"extends": ["@acme/biome-config"]` was reported as an unused dev dependency. The Biome plugin now reads `extends` as a string or an array, and credits the package name without its subpath, so `@acme/biome-config/strict` credits `@acme/biome-config`. A file path (an entry that starts with `.` or `/`, or ends in `.json` or `.jsonc`) and the Biome 2 root marker `//` credit no package.
