### Added

- **Shallow directories in `fallow health`.** The opt-in `fallow health --shallow-directories` lists directories whose interface is as large as what they hide.
  - A directory is shallow when files outside it use at least as many of its interface items as it keeps inside. It must also show at least `health.shallowDirectories.minInterface` items (default 8).
  - An interface item is a module-scope function or an exported class with a method. Types, constants, enums, UI components and exports that a package entry re-exports do not count.
  - Only runtime imports from files that are not tests or stories count as use.
  - Catalogs (about one item per file), families of functions with one parameter list, and items with about one outside user are not reported.
  - Each entry lists the 7 items with the most outside users. `health.shallowDirectories.ignore` hides a directory that is wide on purpose.
  - The section is advisory. It has no health score weight and never fails a run.
  - JSON, SARIF (`fallow/shallow-directory`, level `note`), CodeClimate, compact, markdown, human output and the MCP `check_health` tool (`shallow_directories`) carry it.
  (Closes [#5195](https://github.com/fallow-rs/fallow/issues/5195))
