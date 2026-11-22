### Fixed

- **Review comments land on files with brackets in their path.** The CodeClimate path of a finding encodes `[` and `]` as `%5B` and `%5D`. The review envelope and the PR comment used that encoded path, so the diff filter dropped findings in a route such as `app/[id]/page.tsx` and a review comment could not anchor. CI issues now carry the plain file path.

- **`fallow audit` inherits the findings of a renamed file whose path holds `%` or `:`.** A dead-code key escapes these characters, and the rename map holds plain paths, so the remap and the moved-export match missed such a file and reported its findings as introduced.

- **`fallow audit --brief` reports only new public exports when dead code and health use different production modes.** The base pass then kept no module graph, so every export of a changed public file read as added in `deltas.public_api_added`.

- **The typed decision surface reads a moved `package.json` under its base path.** `fallow_api::run_decision_surface` and the `decision_surface` MCP tool compared a moved manifest with nothing, so every dependency of the moved package read as a new dependency decision. The decision ids of a renamed file now also carry the previous signal id, as the CLI brief does.
