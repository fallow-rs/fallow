### Fixed

- **`unused-load-data-key` no longer reports the response body of a resource route.** A React Router or Remix route module with a `loader` and no default export component is a resource route. An HTTP client reads its response, so no component can read the keys. The rule now skips such a route. It also skips the keys of a `Response.json(...)` return. The rule text in `fallow explain`, SARIF, CodeClimate, LSP, the GitHub annotations and job summary, and the skill references now describes SvelteKit, React Router and Remix loaders. The parse cache version changes, so the first run after the upgrade parses all files again.
  (Closes [#5264](https://github.com/fallow-rs/fallow/issues/5264))
