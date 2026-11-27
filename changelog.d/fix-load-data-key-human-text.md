### Fixed

- **The human and Markdown lines of `unused-load-data-key` now name React Router and Remix loaders.** Each line says that the key is returned from a route `load()` or `loader`, and names the `data.<key>`, `page.data.<key>` and `useLoaderData()` reads. The line no longer describes a SvelteKit-only rule.
  (Refs [#5264](https://github.com/fallow-rs/fallow/issues/5264))
