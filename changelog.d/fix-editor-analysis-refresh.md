### Fixed

- **The language server watches Vue, Svelte, Astro, and MDX files.** A client that supports file watchers now reports external edits to those files so the server can update its diagnostics.
- **A refreshed analysis session reports source skips from the latest file scan.** When a file crosses the size limit, the session adds or clears its diagnostic on the next scan.
