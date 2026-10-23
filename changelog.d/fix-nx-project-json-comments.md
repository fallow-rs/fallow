### Fixed

- **A script that an Nx `run-commands` target runs is no longer reported as an unused file when `project.json` contains a comment.**
  Fallow now reads `project.json` as JSONC, as Nx does. Line comments, block
  comments and trailing commas no longer hide the target commands. Thanks to
  @Pajn for the report and the clear reproduction (#3704).
