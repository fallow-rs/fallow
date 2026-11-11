### Fixed

- **The pull-request comment says "Quality gate failed" when an enforced gate failed.** The diff filter of the comment can drop every finding of the run, for example a circular dependency that starts outside the changed lines. The run then exits 1, but the heading said "Quality gate passed". Now a failed enforced gate in `gate_outcomes` sets the heading. The check-run conclusion does not change.

- **The GitHub Action installs the fallow version of a pnpm or Bun catalog.** A `"fallow": "catalog:"` or `"catalog:<name>"` dependency in `package.json` made the Action install the latest release. Now the Action walks up from the `root` input to the first catalog file and reads the fallow entry of that catalog. The catalog file is a `pnpm-workspace.yaml`, or a `package.json` with `catalog`, `catalogs` or `workspaces.catalog(s)` (Bun). When it cannot find the entry, it prints a warning that names the catalog and tells you to set the `version` input.
