### Fixed

- **More markup forms that pass the whole `data` prop now stop the unused load data key report.** A SvelteKit route component that uses `bind:data`, `data="{data}"` or a template call argument such as `{format(data)}` lets a child read any load key. Fallow now counts each of these forms as a use of the whole `data` object. So does any other plain mention of `data` in markup, for example a `{data}` text node or `{#if data}`. For such a route, fallow reports no unused load data keys. A `data` mention inside script text no longer counts. The extraction cache version changes.
  (Refs [#5171](https://github.com/fallow-rs/fallow/issues/5171))
