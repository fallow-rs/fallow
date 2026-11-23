### Fixed

- **The language server watches all source extensions that Fallow analyzes.** Clients that support file watchers now report external changes to component files, stylesheets, HTML, and GraphQL so diagnostics refresh after edits and branch switches.
- **A refreshed analysis session reports source skips from the latest file scan.** When a file crosses the size limit, the session adds or clears its diagnostic on the next scan.
