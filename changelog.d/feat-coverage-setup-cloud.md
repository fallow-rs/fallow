### Added

- **`fallow coverage setup --cloud` plans a Fallow Cloud setup in one command.** Fallow Cloud joins runtime coverage to the static inventory only when the beacon, the inventory, the source maps and the deployment registration carry the same commit SHA. The plan gives each step that SHA:
  - The beacon snippets send coverage over HTTP with `commitSha` from the platform env (`FALLOW_COMMIT_SHA`, `VERCEL_GIT_COMMIT_SHA`, `RENDER_GIT_COMMIT`, `RAILWAY_GIT_COMMIT_SHA`, or `VITE_FALLOW_COMMIT_SHA` for a browser bundle) and `coverageOrigin: "production"`.
  - The new `cloud.ci.steps` are GitHub Actions steps: a checkout with `fetch-depth: 0`, `upload-inventory`, `upload-source-maps` for each bundled workspace member, and the deployment registration, all for `$GITHUB_SHA`. In a workspace with more members, each beacon gets a `runtimeSurface` tag.
  - The plan names the key variables (`FALLOW_API_KEY`, `VITE_FALLOW_API_KEY`), never a key value, and it writes nothing. A project id, `--git-sha` or `--api-endpoint` that is not safe in a shell command exits 2.
  - `--json` adds the `cloud` object to the setup envelope. Without `--cloud` the envelope does not change.
  - `--wait` polls the Fallow Cloud activation endpoint for the SHA until every step is done. It retries a network error, a rate limit or a server error. It exits 0 when the setup is complete, 1 on a timeout (`--wait-timeout`, default 600 seconds), and 2 on an auth, key scope or input error. The last activation body goes into `cloud.wait.activation` unchanged.
