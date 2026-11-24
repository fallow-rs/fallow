### Added

- **An `mkdocs` plugin for MkDocs and Zensical sites.** When the project has a `mkdocs.yml`, `mkdocs.yaml` or `zensical.toml`, also in a subdirectory such as `docs/`, each local file in `extra_javascript` and `extra_css` is an entry point. fallow no longer reports these files, and the modules that they import, as unused. A path resolves against `docs_dir` (default `docs`). Zensical reads the keys from the `[project]` table. A URL entry adds no entry point.
  (Closes [#5161](https://github.com/fallow-rs/fallow/issues/5161))
