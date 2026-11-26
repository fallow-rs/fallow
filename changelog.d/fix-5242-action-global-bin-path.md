### Fixed

- **The GitHub Action and the GitLab template run the fallow command that npm installed, also when the npm global bin folder is not on `PATH`.** Node from `pnpm/setup` puts only the pnpm folders on `PATH`. The install step then reported "unknown version", and the next step failed with `fallow: command not found`. The install step now puts the npm global bin folder first on `PATH` for the later steps when `fallow` is missing from `PATH` or is a different file. When npm installed no fallow command, the install step fails with an error that names the folder.
  (Closes [#5242](https://github.com/fallow-rs/fallow/issues/5242))
