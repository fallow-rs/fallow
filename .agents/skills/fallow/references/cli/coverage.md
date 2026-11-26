# `coverage`: Production-Coverage Workflow

Use the `coverage` subcommands for runtime setup, analysis, inventory upload, and scoped cloud reads:

- `coverage setup` - resumable state machine that wires sidecar installation, framework-aware coverage recipe writing, optional license activation for continuous monitoring, and automatic handoff into `fallow health --runtime-coverage`.
- `coverage analyze` - focused runtime coverage analysis. Local mode reads `--runtime-coverage <path>`; cloud mode requires explicit `--cloud`, `--runtime-coverage-cloud`, or `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud` and never triggers from `FALLOW_API_KEY` alone.
- `coverage upload-inventory` - push a static function inventory to Fallow Cloud so the dashboard can surface `untracked` functions (those in the codebase but never called at runtime).
- `coverage review-packet` - read the production facts of changed files or functions from Fallow Cloud as JSON, with no local analysis and no full runtime-context pull. `--file <path>` and `--function <file>:<name>[:<line>]` select the scope; with neither, the source files changed against `--base` (resolved like `fallow audit`) are sent.
- `coverage deployment-changes` - read the Fallow Cloud deployment change report for `--sha` (default `HEAD`) against `--base` (default: the previous deployment with production runtime) as JSON.

```bash
fallow coverage setup                         # interactive
fallow coverage setup --yes                   # accept all prompts
fallow coverage setup --non-interactive       # print instructions, do not prompt
fallow coverage setup --yes --json            # agent-readable JSON, no prompts/writes/installs/network
fallow coverage setup --yes --json --explain  # add _meta field docs, enums, warnings, docs URL

fallow coverage analyze --runtime-coverage ./coverage --format json
fallow coverage analyze --cloud --repo owner/repo --format json

fallow coverage review-packet --repo owner/repo                  # files changed against the merge-base
fallow coverage review-packet --repo owner/repo --file src/api.ts
fallow coverage deployment-changes --repo owner/repo --sha <sha> --base <sha>

fallow coverage upload-inventory              # infers project-id, git-sha, API key
fallow coverage upload-inventory --dry-run    # print what would be uploaded, exit 0

fallow coverage upload-source-maps --dir dist           # upload build source maps from CI
fallow coverage upload-source-maps --dry-run            # print maps and fileName values, no network
```

`--json` is the agent-driven entry point: implies `--non-interactive`, never writes files, never installs the sidecar, never makes network calls, and produces a stable JSON payload with these top-level keys: `schema_version` (string `"1"`), `framework_detected`, `package_manager`, `runtime_targets`, `members`, `config_written`, `commands`, `files_to_edit`, `snippets`, `dockerfile_snippet`, `next_steps`, `warnings`. Add `--explain` to inject an opt-in `_meta` block with field definitions, enum values, warning semantics, and the docs URL; `schema_version` stays `"1"`. `framework_detected` uses canonical ids (`nextjs`, `nestjs`, `nuxt`, `sveltekit`, `astro`, `remix`, `vite`, `plain_node`, `unknown`). When both a Node-server framework (Elysia, Hono, Fastify, Express, Koa, `@trpc/server`) and Vite appear in the same `package.json`, the Node-server framework wins. Workspace projects emit one `members[]` entry per runtime-bearing workspace (each with its own `framework_detected`, `package_manager`, `runtime_targets`, `files_to_edit`, `snippets`, `dockerfile_snippet`, `warnings`); top-level fields mirror the first emitted runtime member, and `runtime_targets` at top level is the union (`[]`, `["node"]`, `["browser"]`, or `["node", "browser"]`) across all members. Single-app projects emit a `members[]` array of length 1 (path `"."`) so consumers can treat it uniformly. Library-only workspaces (no `start`/`preview`/`dev` script and no Node-server dependency) are skipped, as are aggregator roots whose only `dev` / `preview` script delegates to a tool other than vite (e.g., `turbo dev`, `nx run-many`); when no runtime members are found, the payload reports `framework_detected: "unknown"`, `runtime_targets: []`, `members: []`, and a `warnings` entry of `"No runtime workspace members were detected; emitted install commands only."`. A Vite browser app is recognized when `vite` is a dependency AND either a `dev`/`preview` script invokes `vite` (or `vite-preview` / `vite-plus` / `vp`) OR the workspace contains an `index.html` or `src/main.{ts,tsx,js,jsx,mts,mjs}` entry.

## `setup` flow

1. **License check** - if missing or hard-fail, offers to start a trial.
2. **Sidecar discovery** - resolves `FALLOW_COV_BIN`, `FALLOW_COV_BINARY_PATH`, platform-package binaries in npm/bun/pnpm layouts, project-local `node_modules/.bin/fallow-cov`, package-manager bin, `~/.fallow/bin/fallow-cov`, and `PATH`. When an explicit env path is set but points to a non-existent file, setup errors fast instead of falling through.
3. **Coverage recipe** - detects framework (Next.js, Nuxt, Astro, SvelteKit, Remix, NestJS, Vite browser apps, plain Node) and package manager (npm, pnpm, yarn, bun), then writes `docs/collect-coverage.md` with the correct commands.
4. **Handoff** - if `./coverage/coverage-final.json` or a V8 coverage directory already exists, setup runs `fallow health --runtime-coverage <path>` directly.

## `analyze` flags

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--runtime-coverage <PATH>` | path | none | Local V8 directory, V8 JSON file, or Istanbul coverage map. Mutually exclusive with cloud mode. |
| `--cloud`, `--runtime-coverage-cloud` | bool | false | Explicitly fetch cloud runtime facts from `/v1/coverage/:repo/runtime-context`. |
| `--api-key <KEY>` | string | `$FALLOW_API_KEY` | Fallow Cloud bearer token, used only after explicit cloud opt-in. |
| `--api-endpoint <URL>` | string | `$FALLOW_API_URL` or `https://api.fallow.cloud` | Override for staging / on-prem. |
| `--repo <OWNER/REPO>` | string | `$FALLOW_REPO`, then parsed git origin | Repository whose latest cloud runtime facts should be pulled. Slashes are percent-encoded as one route segment. |
| `--coverage-period <DAYS>` | integer | 30 | Cloud observation window, 1 through 90 days. |
| `--project-id <ID>` | string | none | Optional project discriminator for monorepos. |
| `--environment <NAME>` | string | none | Optional environment filter. |
| `--commit-sha <SHA>` | string | none | Optional advanced filter for a specific observed commit. |
| `--top <N>` | integer | unset | Show only the top N runtime findings, hot paths, blast-radius entries, and importance entries. Truncation happens before rendering, so it propagates to JSON, human, and cloud-merge output equally. |
| `--blast-radius` | bool | false | Show the first-class blast-radius section in human output. JSON always includes `runtime_coverage.blast_radius` whenever runtime coverage analysis runs. |
| `--importance` | bool | false | Show the first-class importance section in human output. JSON always includes `runtime_coverage.importance` whenever runtime coverage analysis runs. |
| `--debug-unmatched` | bool | false | Cloud mode only. List every cloud runtime function with no local counterpart on stderr, highest traffic first, instead of only counting them in the `cloud_functions_unmatched` warning. Stdout stays machine-readable. |
| `--production` | bool | false | Run analyze in production mode, matching `fallow health --production`. Filters out test files and dev-only code paths before merging runtime data. |
| `--min-invocations-hot <N>` | integer | 100 | Hot-path classification threshold. Functions invoked at least N times during the captured window are classified as hot. Mirrors the same flag on `fallow health --runtime-coverage`. |
| `--min-observation-volume <N>` | integer | 5000 | Minimum total trace volume before the sidecar emits high-confidence `safe_to_delete` / `review_required` verdicts. Below this, confidence is capped at `medium`. |
| `--low-traffic-threshold <RATIO>` | decimal | 0.001 | Fraction of total trace count below which an invoked function is classified `low_traffic` rather than `active`. `0.001` = 0.1%. |
| `--explain` | bool | false | With `--format json`, attach a top-level `_meta` block with field definitions, enum values (`data_source`, `test_coverage`, `v8_tracking`, `action_type`, etc.), warning-code documentation, and the docs URL. |

Cloud analysis emits the same `runtime_coverage` JSON block as local mode. Its summary includes `data_source: "cloud"`, `last_received_at`, and `capture_quality` derived from the pulled runtime window. Cloud functions that cannot be matched to the local AST/static index are omitted from findings and reported through a `cloud_functions_unmatched` warning. Matching resolves a runtime file path onto the local tree by file name plus a segment-wise suffix comparison, so a containerized `/app/src/a.ts` reaches `src/a.ts`, and a function whose runtime name differs from the source name (an anonymous callback carrying its callee's name, an accessor keeping its `get` prefix) is matched on position within that file. Ambiguity is never guessed: two local files equally entitled to a runtime path, or two definitions opening on one line with no end line to separate them, stay unmatched. Pass `--debug-unmatched` to list what remains.

Each finding's `actions[].type` uses the canonical kebab-case vocabulary: `delete-cold-code` is emitted on `verdict=safe_to_delete`, `review-runtime` on `verdict=review_required`. The sidecar may emit additional protocol-specific identifiers, so consumers should treat unknown values as forward-compat extensions rather than schema violations.

Under `--production` the evidence block also carries `test_only_reference`. It is `true` when the function is unreachable in the production module graph but still referenced from a file production mode excludes (test, spec, story, fixture, benchmark). Such a function is never `safe_to_delete`: it is reported as `review_required` with the action "Only tests reference this export; delete the test usage together with the function or keep it". The field is absent when no production filter was applied, because there is no second reachability answer to report.

## `upload-inventory` flags

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--api-key <KEY>` | string | `$FALLOW_API_KEY` | Fallow Cloud bearer token. Generate at `https://fallow.cloud/settings#api-keys`. **Prefer `$FALLOW_API_KEY` on shared CI runners**: `--api-key` on the command line may be visible to other processes via `ps`. |
| `--api-endpoint <URL>` | string | `$FALLOW_API_URL` or `https://api.fallow.cloud` | Override for staging / on-prem. |
| `--project-id <OWNER/REPO>` | string | `$GITHUB_REPOSITORY` → `$CI_PROJECT_PATH` → `git remote get-url origin` | Project identifier. |
| `--git-sha <SHA>` | string | `git rev-parse HEAD` | Commit SHA this inventory is keyed to. Max 64 chars; `[A-Za-z0-9._-]` only. |
| `--allow-dirty` | bool | `false` | Silence the warning when the working tree has uncommitted changes. |
| `--exclude-paths <GLOB>` | glob | none | Additional globs to skip (repeatable), applied after the configured fallow ignore rules. |
| `--path-prefix <PREFIX>` | string | none | Prefix prepended to every emitted `filePath` so inventory matches runtime paths. Required for containerized deployments (runtime reports `/app/src/*` while the walker emits `src/*`). Common values: `/app`, `/workspace`, `/usr/src/app`, `/var/task`, `/home/runner/work/<repo>/<repo>`. Must start with `/`. |
| `--dry-run` | bool | `false` | Print what would be uploaded and exit. No network call. |
| `--ignore-upload-errors` | bool | `false` | Treat upload failures as warnings (exit 0). Validation errors still fail hard. |

Only plain JS/TS/JSX/TSX sources are walked. Declaration files (`*.d.ts`, `*.d.mts`, `*.d.cts`, `*.d.tsx`) and bodyless function signatures (TS overloads, `abstract` methods, `declare function`) are intentionally skipped; they have no runtime footprint. Function names match `oxc-coverage-instrument` byte-for-byte so the join with runtime coverage succeeds.

## Environment

- `FALLOW_COV_BIN` - explicit override for the sidecar binary (for `setup`). Wins over all other discovery paths. Must point to an existing file.
- `FALLOW_API_KEY` - Fallow Cloud bearer token (for `upload-inventory` and `upload-source-maps`). Overridden by `--api-key` for `upload-inventory`; `upload-source-maps` reads only the env var so secrets stay out of argv.
- `FALLOW_API_URL` - base URL for cloud calls. Overridden by `--api-endpoint`.
- `FALLOW_CA_BUNDLE` - PEM certificate bundle for cloud calls. Relative paths resolve from the process cwd. The bundle replaces default WebPKI roots, so private-CA runners should pass a complete bundle that includes public roots plus the private CA.

## `coverage upload-source-maps` flags

Coverage CI helper for bundled/minified runtime coverage. It scans a build directory for `.map` files and uploads them to `/v1/coverage/:repo/source-maps` keyed by the commit SHA the beacon reports.

Uploads retry network failures, HTTP 429, and HTTP 502/503/504 up to three attempts. HTTP 429 honors `Retry-After` delta seconds and HTTP-date values, capped at 60 seconds. Setup or transport failures that prevent every map from uploading exit 7; mixed per-map failures still exit 1.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--dir <PATH>` | path | `dist` | Directory scanned recursively. |
| `--include <GLOB>` | glob | `**/*.map` | Include glob relative to `--dir`. |
| `--exclude <GLOB>` | glob | `**/node_modules/**` | Exclude glob, repeatable. |
| `--repo <NAME>` | string | `package.json` `repository.url`, then `git remote get-url origin` parsed to `owner/repo` | Repo identifier used in the source-map API path. Must match the beacon's `projectId` (and `upload-inventory`'s `--project-id`); pass `--repo <bare-name>` explicitly if the beacon reports a bare name. |
| `--git-sha <SHA>` | string | `$GITHUB_SHA` -> `$CI_COMMIT_SHA` -> `$COMMIT_SHA` -> `git rev-parse HEAD` | Commit SHA, 7-40 hex chars. |
| `--endpoint <URL>` | string | `$FALLOW_API_URL` or `https://api.fallow.cloud` | Override for staging / on-prem. |
| `--strip-path <BOOL>` | bool | `true` | Upload basename-only `fileName` values. Use `--strip-path=false` when runtime coverage reports paths like `assets/app.js`. Maps that share a basename are sent under their build-directory path, so no map replaces another. The command warns when a map's `sources` do not resolve to files in the repository (for example `sourceRoot: "/"` in `tsconfig.json`). |
| `--dry-run` | bool | `false` | Print what would upload; no API key or network call. |
| `--concurrency <N>` | integer | `4` | Parallel upload fanout. |
| `--fail-fast` | bool | `false` | Stop on the first upload failure. |

## Exit Codes

| Code | Meaning |
|------|---------|
| `0`  | Setup complete / upload succeeded / dry-run printed |
| `2`  | Bad invocation, unable to resolve sidecar via env override (`setup`) |
| `4`  | Sidecar install failed (`setup`) |
| `5`  | Coverage input could not be pre-processed (`setup`) |
| `7`  | Network failure (trial activation for `setup`; upload DNS/TLS/connect for `upload-inventory`) |
| `10` | Validation error: missing API key, unresolvable project-id, zero functions (`upload-inventory`) |
| `11` | Payload too large: inventory exceeds the 200,000-function server cap (`upload-inventory`) |
| `12` | Auth rejected: 401 / 403 from the server (`upload-inventory`) |
| `13` | Server error: 5xx or other non-2xx status (`upload-inventory`) |
