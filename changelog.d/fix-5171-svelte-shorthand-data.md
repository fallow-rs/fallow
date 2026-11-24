### Fixed

- **SvelteKit shorthand `{data}` attributes (`<View {data} />`) now count as whole-`data` passes.** Just like `data={data}` and `{...data}`, forwarding `data` via shorthand attribute lets the child component read any key returned by `load()`, so `unused-load-data-key` now abstains for that route (`CACHE_VERSION` bumped to invalidate stale parse caches).
  (Closes [#5171](https://github.com/fallow-rs/fallow/issues/5171))
