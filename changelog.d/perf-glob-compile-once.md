### Fixed

- **A config or plugin glob builds its regex once again.** Since #5467, each glob with a `{` group, for example the plugin entry glob `app/**/page.{ts,tsx,js,jsx}`, built its regex two times: once as a check and once for the matcher. In a monorepo with many workspaces, analysis used up to 40% more CPU. The matcher now builds the regex one time and returns a compile failure as an error, so a glob that does not compile is still a config error and not a crash.
