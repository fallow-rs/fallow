### Added

- **Built-in vite-plugin-pages plugin.** Fallow now reads the `Pages(...)` call in `vite.config.*` and makes the page files under each `dirs` entry entry points. A `dirs` entry can be a string or an object with `dir`, and can point into another workspace package (`"../common/src/pages"`). Without `dirs`, the page directory is `src/pages`. The `extensions` option sets the page file extensions. Before, fallow reported the page files and the files that only pages import as unused files.
