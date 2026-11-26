### Fixed

- **The GitHub Action and the GitLab template find the fallow command when the npm global bin folder is not on `PATH`.** Node from `pnpm/setup` puts only the pnpm folders on `PATH`. The install step then reported "unknown version", and the next step failed with `fallow: command not found`. The install step now adds the npm global bin folder to `PATH` for the later steps. When npm installed no fallow command, the install step fails with an error that names the folder.
  (Closes [#5242](https://github.com/fallow-rs/fallow/issues/5242))
